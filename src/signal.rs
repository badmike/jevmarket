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
    pub no_bid: Option<f64>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Outcome {
    Yes,
    No,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "YES",
            Self::No => "NO",
        }
    }
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
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
    Skip(Skip),
}

impl Verdict {
    fn skip(code: SkipCode, reason: impl Into<String>) -> Self {
        Self::Skip(Skip { code, reason: reason.into() })
    }
}

/// Why a market was not traded: a code the console can explain, and the detail for the log.
#[derive(Debug, Clone)]
pub struct Skip {
    pub code: SkipCode,
    pub reason: String,
}

/// Stored in `decisions.skip_code`; `web/src/api/types.ts` lists the same codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipCode {
    /// The resolution rules are too vague (also the pre-screen before research).
    Unclear,
    /// Jev lacks the information to judge the question.
    Unanswerable,
    NoAsks,
    /// Both asks lie outside `min_trade_price..=max_trade_price`.
    OutsideBand,
    SmallEdge,
    /// An edge above `suspicious_edge`: more likely a model error.
    SuspiciousEdge,
    ZeroStake,
    /// The exchange's minimum order would exceed the per-trade cap.
    MinOrderTooBig,
    /// A trade signal on a cached brief that could not be researched again.
    StaleBrief,
    /// A price-watch signal after a price move, when Jev could not be asked again.
    StaleView,
}

impl SkipCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unclear => "unclear",
            Self::Unanswerable => "unanswerable",
            Self::NoAsks => "no_asks",
            Self::OutsideBand => "outside_band",
            Self::SmallEdge => "small_edge",
            Self::SuspiciousEdge => "suspicious_edge",
            Self::ZeroStake => "zero_stake",
            Self::MinOrderTooBig => "min_order_too_big",
            Self::StaleBrief => "stale_brief",
            Self::StaleView => "stale_view",
        }
    }
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
        return Verdict::skip(
            SkipCode::Unanswerable,
            format!("answerable {:.2} < {}", view.answerable, s.min_answerable),
        );
    }
    if view.clarity < s.min_clarity {
        return Verdict::skip(SkipCode::Unclear, format!("clarity {} < {}", view.clarity, s.min_clarity));
    }

    let sides: Vec<(Outcome, U256, f64, f64)> = [
        book.yes_ask.map(|ask| (Outcome::Yes, book.yes_token_id, view.p_yes, ask)),
        book.no_ask.map(|ask| (Outcome::No, book.no_token_id, 1.0 - view.p_yes, ask)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if sides.is_empty() {
        return Verdict::skip(SkipCode::NoAsks, "no asks on either side");
    }
    let in_band = sides.iter().filter(|(.., ask)| (s.min_trade_price..=s.max_trade_price).contains(ask));
    let Some(&(outcome, token_id, p, ask)) = in_band.max_by(|a, b| (a.2 - a.3).total_cmp(&(b.2 - b.3))) else {
        let asks: Vec<_> = sides.iter().map(|(o, .., ask)| format!("{o} ask {ask:.2}")).collect();
        return Verdict::skip(
            SkipCode::OutsideBand,
            format!("outside trade band [{}, {}]: {}", s.min_trade_price, s.max_trade_price, asks.join(", ")),
        );
    };

    let edge = p - ask;
    if edge < s.min_edge {
        return Verdict::skip(
            SkipCode::SmallEdge,
            format!("best edge {edge:+.3} ({outcome} p={p:.2} ask={ask:.2}) < {}", s.min_edge),
        );
    }

    // Sizing: fractional Kelly on the exposure cap as bankroll, hard-capped per trade.
    let usd = s.max_usd_per_trade.min(kelly_fraction(p, ask) * s.kelly_fraction * s.max_open_exposure_usd);
    if usd <= 0.0 {
        return Verdict::skip(SkipCode::ZeroStake, "kelly sizing gave zero");
    }
    let (price, size, usd) = sized(book, ask, usd);
    if usd > s.max_usd_per_trade * 1.5 {
        // The exchange minimum pushed the order well past the cap; refuse.
        return Verdict::skip(
            SkipCode::MinOrderTooBig,
            format!("min order size {} x {price} = ${usd:.2} exceeds cap", book.min_order_size),
        );
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

/// The highest ask at which [`evaluate`] buys a side Jev gives probability `p`: on the book's
/// tick, inside the trade band and at least `min_edge` below `p`, with the same float comparison.
/// `None` when no ask in the band leaves that edge. Sizing is left out.
pub fn trigger_price(p: f64, tick: f64, s: &Settings) -> Option<f64> {
    let tick = if tick > 0.0 { tick } else { 0.01 };
    let highest = (p - s.min_edge).min(s.max_trade_price);
    let mut ask = round_to_tick((highest / tick + 1e-9).floor() * tick, tick);
    if p - ask < s.min_edge {
        ask = round_to_tick(ask - tick, tick);
    }
    (s.min_trade_price..=s.max_trade_price).contains(&ask).then_some(ask)
}

/// Shares the wallet holds in one outcome of a market.
#[derive(Debug, Clone)]
pub struct Holding {
    pub outcome: Outcome,
    pub token_id: U256,
    pub size: f64,
    /// What a share cost on average.
    pub avg_price: f64,
}

/// A SELL of a whole holding at the best bid, before the market resolves.
#[derive(Debug, Clone)]
pub struct Exit {
    pub outcome: Outcome,
    pub token_id: U256,
    /// Limit price: the best bid.
    pub price: f64,
    pub size: f64,
    /// Proceeds, `price * size`.
    pub usd: f64,
    /// `price - p`: how much a share sold now beats what Jev expects it to pay at resolution.
    pub edge: f64,
    pub rationale: String,
}

impl Holding {
    fn p(&self, view: &JevView) -> f64 {
        match self.outcome {
            Outcome::Yes => view.p_yes,
            Outcome::No => 1.0 - view.p_yes,
        }
    }
}

/// Float slack on the exit limits, so a bid of 0.45 over a probability of 0.40 meets a 0.05 margin.
const EXIT_TOLERANCE: f64 = 1e-9;

/// Sell a holding early only when both hold: the best bid beats Jev's probability of the held side
/// by `min_exit_edge` (the market pays more now than Jev expects the shares to be worth at
/// resolution), and it returns at least `min_exit_profit` on what the shares cost. A view that
/// fails the answerable or clarity gate is not trusted to sell on. Otherwise the reason to hold.
pub fn evaluate_exit(view: &JevView, book: &Book, h: &Holding, s: &Settings) -> Result<Exit, String> {
    if view.answerable < s.min_answerable || view.clarity < s.min_clarity {
        return Err("Jev's view fails the answerable or clarity gate".into());
    }
    let bid = match h.outcome {
        Outcome::Yes => book.yes_bid,
        Outcome::No => book.no_bid,
    };
    let bid = bid.ok_or("nobody is buying")?;
    let (p, outcome) = (h.p(view), h.outcome);
    let edge = bid - p;
    if edge < s.min_exit_edge - EXIT_TOLERANCE {
        return Err(format!("bid {bid:.2} is {edge:+.3} over Jev's P({outcome})={p:.2}, below {}", s.min_exit_edge));
    }
    if h.avg_price <= 0.0 || bid < h.avg_price * (1.0 + s.min_exit_profit) - EXIT_TOLERANCE {
        return Err(format!("bid {bid:.2} on a {:.2} entry returns less than {}", h.avg_price, s.min_exit_profit));
    }
    let size = (h.size * 100.0).floor() / 100.0;
    if size < book.min_order_size {
        return Err(format!("{size} shares are below the minimum order of {}", book.min_order_size));
    }
    Ok(Exit {
        outcome,
        token_id: h.token_id,
        price: bid,
        size,
        usd: (bid * size * 1e4).round() / 1e4,
        edge,
        rationale: format!(
            "sell {outcome} at bid {bid:.2}: Jev P({outcome})={p:.2}, edge {edge:+.2}, bought at {:.2}",
            h.avg_price
        ),
    })
}

/// The lowest bid at which [`evaluate_exit`] sells a holding Jev gives probability `p`, on the
/// book's tick. `None` when no bid below 1 would.
pub fn exit_trigger(p: f64, avg_price: f64, tick: f64, s: &Settings) -> Option<f64> {
    let tick = if tick > 0.0 { tick } else { 0.01 };
    let passes = |bid: f64| {
        bid - p >= s.min_exit_edge - EXIT_TOLERANCE
            && avg_price > 0.0
            && bid >= avg_price * (1.0 + s.min_exit_profit) - EXIT_TOLERANCE
    };
    let lowest = (p + s.min_exit_edge).max(avg_price * (1.0 + s.min_exit_profit));
    let mut bid = round_to_tick((lowest / tick - 1e-9).ceil() * tick, tick);
    if !passes(bid) {
        bid = round_to_tick(bid + tick, tick);
    }
    (bid < 1.0 && passes(bid)).then_some(bid)
}

/// A BUY a person asked for: about `usd` of `outcome` at limit `price`. Jev's probability plays
/// no part, so the edge is zero; the executor's money caps still apply.
pub fn manual_trade(book: &Book, outcome: Outcome, price: f64, usd: f64) -> Result<Trade, String> {
    if !(price > 0.0 && price < 1.0) {
        return Err(format!("price {price} must be between 0 and 1"));
    }
    if !usd.is_finite() || usd <= 0.0 {
        return Err(format!("amount ${usd} must be positive"));
    }
    let (price, size, usd) = sized(book, price, usd);
    if !(price > 0.0 && price < 1.0) {
        return Err(format!("price rounds to {price} on a {} tick", book.tick_size));
    }
    let token_id = match outcome {
        Outcome::Yes => book.yes_token_id,
        Outcome::No => book.no_token_id,
    };
    Ok(Trade { outcome, token_id, price, size, usd, edge: 0.0, rationale: "manual order".into() })
}

/// `(price, size, usd)` for about `usd` at `price`: the price on the tick, whole cents of shares,
/// at least the book's minimum size.
fn sized(book: &Book, price: f64, usd: f64) -> (f64, f64, f64) {
    let price = round_to_tick(price, book.tick_size);
    let size = ((usd / price * 100.0).floor() / 100.0).max(book.min_order_size);
    (price, size, (price * size * 1e4).round() / 1e4)
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
            no_bid: Some(((1.0 - yes_ask) * 1e4).round() / 1e4),
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
            Verdict::Skip(why) => panic!("expected a trade, got skip: {}", why.reason),
        }
    }

    fn skip(v: Verdict) -> String {
        match v {
            Verdict::Skip(why) => why.reason,
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
    fn manual_trade_rounds_like_the_signal() {
        let t = manual_trade(&book(0.40, 0.38, 5.0), Outcome::No, 0.617, 4.0).unwrap();
        assert_eq!((t.token_id, t.price, t.size), (NO, 0.62, 6.45));
        assert_eq!(manual_trade(&book(0.40, 0.38, 5.0), Outcome::Yes, 0.40, 1.0).unwrap().size, 5.0, "min size");
        assert!(manual_trade(&book(0.40, 0.38, 5.0), Outcome::Yes, 1.2, 4.0).is_err());
        assert!(manual_trade(&book(0.40, 0.38, 5.0), Outcome::Yes, 0.004, 4.0).is_err(), "rounds to zero");
        assert!(manual_trade(&book(0.40, 0.38, 5.0), Outcome::Yes, 0.4, 0.0).is_err());
    }

    #[test]
    fn trigger_is_the_highest_ask_evaluate_buys() {
        let s = Settings::default();
        let yes_only = |ask: f64| Book { no_ask: None, ..book(ask, ask - 0.01, 5.0) };
        let buys = |p: f64, ask: f64| matches!(evaluate(&view(p, 0.9, 3), &yes_only(ask), &s), Verdict::Trade(_));
        for cents in 18..=99 {
            let p = f64::from(cents) / 100.0 + 0.004;
            let trigger = trigger_price(p, 0.01, &s).unwrap();
            assert!(buys(p, trigger), "p={p} buys at {trigger}");
            assert!(!buys(p, trigger + 0.01), "p={p} does not buy above {trigger}");
        }
        assert_eq!(trigger_price(0.65, 0.01, &s), Some(0.57));
        assert_eq!(trigger_price(0.99, 0.01, &s), Some(0.90), "capped by max_trade_price");
        assert_eq!(trigger_price(0.15, 0.01, &s), None, "no ask in the band leaves the edge");
    }

    #[test]
    fn exits_only_when_the_bid_beats_jev_and_pays_off() {
        let s = Settings::default();
        let yes = Holding { outcome: Outcome::Yes, token_id: YES, size: 10.0, avg_price: 0.40 };
        let exit = evaluate_exit(&view(0.58, 0.9, 3), &book(0.66, 0.65, 5.0), &yes, &s).unwrap();
        assert_eq!((exit.token_id, exit.price, exit.size, exit.usd), (YES, 0.65, 10.0, 6.5));
        assert!(
            evaluate_exit(&view(0.62, 0.9, 3), &book(0.66, 0.65, 5.0), &yes, &s).is_err(),
            "Jev still expects more"
        );
        let pricey = Holding { avg_price: 0.60, ..yes.clone() };
        assert!(evaluate_exit(&view(0.50, 0.9, 3), &book(0.66, 0.65, 5.0), &pricey, &s).is_err(), "too little profit");
        assert!(evaluate_exit(&view(0.58, 0.3, 3), &book(0.66, 0.65, 5.0), &yes, &s).is_err(), "untrusted view");
        let no = Holding { outcome: Outcome::No, token_id: NO, ..yes };
        let exit = evaluate_exit(&view(0.50, 0.9, 3), &book(0.35, 0.34, 5.0), &no, &s).unwrap();
        assert_eq!((exit.token_id, exit.price), (NO, 0.65), "NO bid is 1 - YES ask");
    }

    #[test]
    fn exit_trigger_is_the_lowest_bid_evaluate_exit_sells_at() {
        let s = Settings::default();
        let h = |avg_price| Holding { outcome: Outcome::Yes, token_id: YES, size: 10.0, avg_price };
        let sells = |p: f64, avg: f64, bid: f64| {
            let b = Book { yes_bid: Some(bid), ..book(bid + 0.01, bid, 5.0) };
            evaluate_exit(&view(p, 0.9, 3), &b, &h(avg), &s).is_ok()
        };
        for cents in 10..=90 {
            let p = f64::from(cents) / 100.0 + 0.004;
            for avg in [0.2, 0.4, 0.6] {
                let Some(trigger) = exit_trigger(p, avg, 0.01, &s) else { continue };
                assert!(sells(p, avg, trigger), "p={p} avg={avg} sells at {trigger}");
                assert!(!sells(p, avg, trigger - 0.01), "p={p} avg={avg} holds below {trigger}");
            }
        }
        assert_eq!(exit_trigger(0.58, 0.40, 0.01, &s), Some(0.63));
        assert_eq!(exit_trigger(0.97, 0.40, 0.01, &s), None);
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
