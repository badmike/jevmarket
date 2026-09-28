//! Turn a market state into a Jev view, and a Jev view + order book into a trade (or not).
//!
//! [`evaluate`] is a pure function so the sizing and threshold logic is testable without
//! touching Jev or Polymarket.

use std::sync::LazyLock;

use indexmap::IndexMap;
use polymarket_client_sdk_v2::types::U256;
use serde_json::Value;

use crate::config::Settings;
use crate::jev::{JevClient, Question, Questions};
use crate::openrouter::OpenRouterError;

const CLARITY_LEVELS: [&str; 5] = [
    "Resolution criteria are missing, contradictory, or depend on undefined terms.",
    "Criteria exist but leave major ambiguity about what counts or who decides.",
    "Criteria are mostly clear; a few edge cases are unspecified.",
    "Criteria are clear with a named resolution source; minor edge cases only.",
    "Criteria are precise, objective, and leave no room for interpretation.",
];

/// The three questions asked about every market, in one Jev call.
static QUESTIONS: LazyLock<Questions> = LazyLock::new(|| {
    IndexMap::from([
        (
            "resolves_yes",
            Question::noul(
                "Given the market question, its description and resolution rules, this market will resolve YES. \
                 If an `evidence` brief is present, weigh its dated facts and latest development against \
                 `days_until_resolution`.",
            ),
        ),
        (
            "answerable",
            Question::noul(
                "The state (including the `evidence` brief, if present) contains enough current and relevant \
                 information to form a well-informed probability estimate for this question. This is about \
                 information sufficiency, not certainty: an uncertain outcome can still be well-informed. \
                 Answer NO only if key facts needed to estimate it are missing, stale, or would require news \
                 that is not in the state.",
            ),
        ),
        (
            "clarity",
            Question::score("How clear and objective are the resolution criteria for this market?", CLARITY_LEVELS),
        ),
    ])
});

/// Only the clarity question, for the pre-screen before research is paid for.
static CLARITY_ONLY: LazyLock<Questions> =
    LazyLock::new(|| IndexMap::from([("clarity", QUESTIONS["clarity"].clone())]));

/// Jev's clarity rating of a market, from [`ask_clarity`].
#[derive(Debug, Clone)]
pub struct ClarityView {
    pub clarity: u8,
    pub model: Option<String>,
    pub cost: f64,
    pub raw: Value,
}

#[derive(Debug, Clone)]
pub struct JevView {
    pub p_yes: f64,
    pub answerable: f64,
    pub clarity: u8,
    pub clarity_mean: Option<f64>,
    pub clarity_confidence: Option<f64>,
    pub model: Option<String>,
    pub cost: f64,
    pub raw: Value,
}

/// Best-of-book snapshot for one binary market.
#[derive(Debug, Clone)]
pub struct Book {
    pub yes_token_id: U256,
    pub no_token_id: U256,
    pub yes_bid: Option<f64>,
    pub yes_ask: Option<f64>,
    pub no_ask: Option<f64>,
    pub tick_size: f64,
    pub min_order_size: f64,
}

impl Book {
    pub fn midpoint(&self) -> Option<f64> {
        match (self.yes_bid, self.yes_ask) {
            (Some(bid), Some(ask)) => Some((bid + ask) / 2.0),
            (bid, ask) => ask.or(bid),
        }
    }

    pub fn spread(&self) -> Option<f64> {
        Some(self.yes_ask? - self.yes_bid?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Yes,
    No,
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Yes => "YES",
            Self::No => "NO",
        })
    }
}

/// A BUY of `size` shares of `outcome` at limit `price`.
#[derive(Debug, Clone)]
pub struct Trade {
    pub outcome: Outcome,
    pub token_id: U256,
    /// Limit price, rounded to the tick.
    pub price: f64,
    /// Number of shares.
    pub size: f64,
    /// `price * size`.
    pub usd: f64,
    /// `p - price`.
    pub edge: f64,
    pub rationale: String,
}

#[derive(Debug, Clone)]
pub enum Verdict {
    Trade(Trade),
    Skip(String),
}

pub async fn ask_jev(jev: &JevClient, state: &Value) -> Result<JevView, OpenRouterError> {
    let (d, raw) = jev.decide(state, &QUESTIONS).await?;
    let clarity = d.answer("clarity");
    match (d.answer("resolves_yes").noul(), d.answer("answerable").noul(), clarity.score()) {
        (Some(p_yes), Some(answerable), Some(level)) => Ok(JevView {
            p_yes,
            answerable,
            clarity: level,
            clarity_mean: clarity.score_mean(),
            clarity_confidence: clarity.confidence(),
            model: d.model,
            cost: d.usage.cost,
            raw,
        }),
        _ => Err(OpenRouterError::invalid("Jev", format!("unexpected answer shape: {raw}"))),
    }
}

/// Ask Jev only how clear the resolution criteria are. The criteria do not depend on the
/// evidence, so this runs on the state without a brief, before paying for one.
pub async fn ask_clarity(jev: &JevClient, state: &Value) -> Result<ClarityView, OpenRouterError> {
    let (d, raw) = jev.decide(state, &CLARITY_ONLY).await?;
    match d.answer("clarity").score() {
        Some(clarity) => Ok(ClarityView { clarity, model: d.model, cost: d.usage.cost, raw }),
        None => Err(OpenRouterError::invalid("Jev", format!("unexpected answer shape: {raw}"))),
    }
}

pub fn round_to_tick(price: f64, tick: f64) -> f64 {
    if tick <= 0.0 {
        return (price * 1e4).round() / 1e4;
    }
    ((price / tick).round() * tick * 1e6).round() / 1e6
}

/// Kelly fraction for a binary contract bought at `price` that pays 1 if it wins.
///
/// With net odds `b = (1 - price) / price`, `f* = (p*b - (1-p)) / b = p - (1-p)*price/(1-price)`.
pub fn kelly_fraction(p: f64, price: f64) -> f64 {
    if price <= 0.0 || price >= 1.0 {
        return 0.0;
    }
    (p - (1.0 - p) * price / (1.0 - price)).max(0.0)
}

pub fn evaluate(view: &JevView, book: &Book, s: &Settings) -> Verdict {
    if view.answerable < s.min_answerable {
        return Verdict::Skip(format!("answerable {:.2} < {}", view.answerable, s.min_answerable));
    }
    if view.clarity < s.min_clarity {
        return Verdict::Skip(format!("clarity {} < {}", view.clarity, s.min_clarity));
    }

    let sides: Vec<(Outcome, U256, f64, f64)> = [
        book.yes_ask.map(|ask| (Outcome::Yes, book.yes_token_id, view.p_yes, ask)),
        book.no_ask.map(|ask| (Outcome::No, book.no_token_id, 1.0 - view.p_yes, ask)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if sides.is_empty() {
        return Verdict::Skip("no asks on either side".into());
    }
    let in_band = sides.iter().filter(|(.., ask)| (s.min_trade_price..=s.max_trade_price).contains(ask));
    let Some(&(outcome, token_id, p, ask)) = in_band.max_by(|a, b| (a.2 - a.3).total_cmp(&(b.2 - b.3))) else {
        let asks: Vec<_> = sides.iter().map(|(o, .., ask)| format!("{o} ask {ask:.2}")).collect();
        return Verdict::Skip(format!(
            "outside trade band [{}, {}]: {}",
            s.min_trade_price,
            s.max_trade_price,
            asks.join(", ")
        ));
    };

    let edge = p - ask;
    if edge < s.min_edge {
        return Verdict::Skip(format!("best edge {edge:+.3} ({outcome} p={p:.2} ask={ask:.2}) < {}", s.min_edge));
    }

    // Sizing: fractional Kelly on the exposure cap as bankroll, hard-capped per trade.
    let usd = s.max_usd_per_trade.min(kelly_fraction(p, ask) * s.kelly_fraction * s.max_open_exposure_usd);
    if usd <= 0.0 {
        return Verdict::Skip("kelly sizing gave zero".into());
    }
    let price = round_to_tick(ask, book.tick_size);
    let size = ((usd / price * 100.0).floor() / 100.0).max(book.min_order_size);
    let usd = (price * size * 1e4).round() / 1e4;
    if usd > s.max_usd_per_trade * 1.5 {
        // The exchange minimum pushed the order well past the cap; refuse.
        return Verdict::Skip(format!("min order size {} x {price} = ${usd:.2} exceeds cap", book.min_order_size));
    }

    Verdict::Trade(Trade {
        outcome,
        token_id,
        price,
        size,
        usd,
        edge,
        rationale: format!(
            "Jev P({outcome})={p:.2} vs ask {ask:.2} -> edge {edge:+.2}; answerable={:.2} clarity={}",
            view.answerable, view.clarity
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const YES: U256 = U256::from_limbs([1, 0, 0, 0]);
    const NO: U256 = U256::from_limbs([2, 0, 0, 0]);

    fn book(yes_ask: f64, yes_bid: f64, min_size: f64) -> Book {
        Book {
            yes_token_id: YES,
            no_token_id: NO,
            yes_bid: Some(yes_bid),
            yes_ask: Some(yes_ask),
            no_ask: Some(((1.0 - yes_bid) * 1e4).round() / 1e4),
            tick_size: 0.01,
            min_order_size: min_size,
        }
    }

    fn view(p_yes: f64, answerable: f64, clarity: u8) -> JevView {
        JevView {
            p_yes,
            answerable,
            clarity,
            clarity_mean: Some(clarity.into()),
            clarity_confidence: Some(0.8),
            model: Some("typesafe/jev-1.13".into()),
            cost: 0.0001,
            raw: Value::Null,
        }
    }

    fn trade(v: Verdict) -> Trade {
        match v {
            Verdict::Trade(t) => t,
            Verdict::Skip(why) => panic!("expected a trade, got skip: {why}"),
        }
    }

    fn skip(v: Verdict) -> String {
        match v {
            Verdict::Skip(why) => why,
            Verdict::Trade(t) => panic!("expected a skip, got {t:?}"),
        }
    }

    #[test]
    fn kelly_basic() {
        assert!(kelly_fraction(0.6, 0.4) > 0.0);
        assert_eq!(kelly_fraction(0.3, 0.4), 0.0);
        assert_eq!(kelly_fraction(0.5, 0.0), 0.0);
    }

    #[test]
    fn rounds_to_tick() {
        assert_eq!(round_to_tick(0.4137, 0.01), 0.41);
        assert_eq!(round_to_tick(0.4137, 0.001), 0.414);
    }

    #[test]
    fn buys_yes_when_jev_higher_than_ask() {
        let t = trade(evaluate(&view(0.60, 0.9, 3), &book(0.40, 0.38, 5.0), &Settings::default()));
        assert_eq!(t.outcome, Outcome::Yes);
        assert_eq!(t.token_id, YES);
        assert_eq!(t.price, 0.40);
        assert!(t.usd <= 5.0 * 1.5);
    }

    #[test]
    fn buys_no_when_jev_lower_than_market() {
        let t = trade(evaluate(&view(0.20, 0.9, 3), &book(0.40, 0.38, 5.0), &Settings::default()));
        assert_eq!(t.outcome, Outcome::No);
        assert!((t.edge - (0.8 - 0.62)).abs() < 1e-9, "P(NO)=0.8 against a 0.62 ask");
    }

    #[test]
    fn skips_when_edge_too_small() {
        skip(evaluate(&view(0.45, 0.9, 3), &book(0.40, 0.38, 5.0), &Settings::default()));
    }

    #[test]
    fn skips_when_not_answerable() {
        assert!(
            skip(evaluate(&view(0.6, 0.3, 3), &book(0.40, 0.38, 5.0), &Settings::default())).contains("answerable")
        );
    }

    #[test]
    fn skips_when_unclear() {
        assert!(skip(evaluate(&view(0.6, 0.9, 1), &book(0.40, 0.38, 5.0), &Settings::default())).contains("clarity"));
    }

    #[test]
    fn per_trade_cap_respected() {
        let s = Settings { max_usd_per_trade: 3.0, ..Settings::default() };
        let t = trade(evaluate(&view(0.95, 0.9, 3), &book(0.40, 0.38, 1.0), &s));
        assert!(t.usd <= 3.0 + 0.01);
    }

    #[test]
    fn refuses_when_min_size_blows_cap() {
        let s = Settings { max_usd_per_trade: 3.0, ..Settings::default() };
        skip(evaluate(&view(0.95, 0.9, 3), &book(0.90, 0.88, 100.0), &s));
    }

    #[test]
    fn refuses_longshots_outside_band() {
        // Jev says 15% NO vs a 7-cent ask: "edge" 0.08, but the ask is outside the band.
        let b = book(0.94, 0.93, 5.0);
        assert!(skip(evaluate(&view(0.85, 0.9, 3), &b, &Settings::default())).contains("band"));
        // Widening the band lets it through.
        let wide = Settings { min_trade_price: 0.01, ..Settings::default() };
        assert_eq!(trade(evaluate(&view(0.85, 0.9, 3), &b, &wide)).outcome, Outcome::No);
    }
}
