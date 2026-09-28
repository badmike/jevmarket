//! Discover candidate Polymarket markets and turn them into compact Jev states.

use std::collections::HashMap;

use anyhow::{Context as _, Result, anyhow, bail};
use chrono::{DateTime, Duration, Utc};
use polymarket_client_sdk_v2::clob;
use polymarket_client_sdk_v2::clob::types::request::OrderBookSummaryRequest;
use polymarket_client_sdk_v2::clob::types::response::OrderBookSummaryResponse;
use polymarket_client_sdk_v2::gamma;
use polymarket_client_sdk_v2::gamma::types::request::{EventBySlugRequest, MarketBySlugRequest, MarketsRequest};
use polymarket_client_sdk_v2::types::{Decimal, U256};
use rust_decimal::prelude::ToPrimitive;
use serde_json::{Map, Value, json};

use crate::config::Settings;
use crate::research::Brief;
use crate::signal::Book;
use crate::store::{Resolution, Store};

const PAGE_SIZE: i32 = 50;

/// The fields of a binary Yes/No market the bot uses.
#[derive(Debug, Clone)]
pub struct Market {
    pub slug: String,
    pub question: String,
    pub description: String,
    /// `0x`-prefixed hex, as used by every Polymarket API.
    pub condition_id: String,
    pub end_date: Option<DateTime<Utc>>,
    pub resolution_source: Option<String>,
    pub yes_token: U256,
    pub no_token: U256,
    pub liquidity: f64,
    pub volume: f64,
    pub tick_size: f64,
    pub min_order_size: f64,
}

impl Market {
    pub fn days_to_resolution(&self) -> Option<i64> {
        self.end_date.map(|end| (end - Utc::now()).num_days().max(0))
    }
}

impl TryFrom<&gamma::types::response::Market> for Market {
    type Error = String;

    fn try_from(m: &gamma::types::response::Market) -> Result<Self, String> {
        let outcomes = m.outcomes.as_deref().unwrap_or_default();
        let tokens = m.clob_token_ids.as_deref().unwrap_or_default();
        let (Some(condition_id), [yes_token, no_token]) = (m.condition_id, tokens) else {
            return Err("missing yes/no tokens".into());
        };
        match outcomes {
            [yes, no] if yes.eq_ignore_ascii_case("yes") && no.eq_ignore_ascii_case("no") => {}
            _ => return Err(format!("outcome labels are {outcomes:?}, not Yes/No")),
        }
        Ok(Self {
            slug: m.slug.clone().unwrap_or_default(),
            question: m.question.clone().unwrap_or_default(),
            description: m.description.as_deref().unwrap_or_default().trim().to_owned(),
            condition_id: condition_id.to_string(),
            end_date: m.end_date,
            resolution_source: m.resolution_source.clone().filter(|s| !s.trim().is_empty()),
            yes_token: *yes_token,
            no_token: *no_token,
            liquidity: f(m.liquidity_num).unwrap_or_default(),
            volume: f(m.volume_num).unwrap_or_default(),
            tick_size: f(m.order_price_min_tick_size).unwrap_or(0.01),
            min_order_size: f(m.order_min_size).unwrap_or(5.0),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub market: Market,
    pub book: Book,
}

impl Candidate {
    /// How much a brief on this market could pay off, for ranking research targets. Zero when
    /// neither ask sits in the trade band (no trade is possible); otherwise higher the sooner it
    /// resolves (evidence ages less, capital returns sooner) and the thinner the book (deep
    /// books are priced by many traders already).
    pub fn opportunity(&self, s: &Settings) -> f64 {
        let band = s.min_trade_price..=s.max_trade_price;
        if ![self.book.yes_ask, self.book.no_ask].into_iter().flatten().any(|ask| band.contains(&ask)) {
            return 0.0;
        }
        let days = self.market.days_to_resolution().unwrap_or(s.max_days_to_resolution) as f64;
        1.0 / (1.0 + days / 30.0) / (1.0 + self.market.liquidity / 100_000.0)
    }
}

fn f(x: Option<Decimal>) -> Option<f64> {
    x.and_then(|d| d.to_f64())
}

/// Return a skip reason, or the market if it is a candidate.
fn passes_static_filters(m: &gamma::types::response::Market, s: &Settings) -> Result<Market, String> {
    let yes = |b: Option<bool>| b.unwrap_or(false);
    if !(yes(m.active) && yes(m.accepting_orders)) || yes(m.closed) || yes(m.archived) {
        return Err("not tradable".into());
    }
    if !yes(m.enable_order_book) {
        return Err("no CLOB order book".into());
    }
    let excluded = |label: &str| s.exclude_tags.iter().any(|t| t.eq_ignore_ascii_case(label));
    if let Some(tag) = m.tags.iter().flatten().filter_map(|t| t.label.as_deref()).find(|l| excluded(l)) {
        return Err(format!("tagged {tag}"));
    }
    let market = Market::try_from(m)?;
    if market.liquidity < s.min_liquidity_usd {
        return Err(format!("liquidity ${:.0} < ${:.0}", market.liquidity, s.min_liquidity_usd));
    }
    if market.volume < s.min_volume_usd {
        return Err(format!("volume ${:.0} < ${:.0}", market.volume, s.min_volume_usd));
    }
    let days = market.days_to_resolution().ok_or("no end date")?;
    if days > s.max_days_to_resolution {
        return Err(format!("resolves in {days}d > {}d", s.max_days_to_resolution));
    }
    if let Some(spread) = f(m.spread).filter(|sp| *sp > s.max_spread) {
        return Err(format!("spread {spread:.2} > {}", s.max_spread));
    }
    let yes_price = m.outcome_prices.as_deref().and_then(|p| p.first()).and_then(ToPrimitive::to_f64);
    if let Some(px) = yes_price.filter(|px| !(s.min_market_price..=s.max_market_price).contains(px)) {
        return Err(format!("yes price {px:.3} outside [{}, {}]", s.min_market_price, s.max_market_price));
    }
    Ok(market)
}

fn best(levels: &[clob::types::response::OrderSummary], pick: fn(f64, f64) -> f64) -> Option<f64> {
    levels.iter().filter_map(|l| l.price.to_f64()).reduce(pick)
}

/// Match books to the market's YES/NO tokens by asset id (response order is not guaranteed).
fn book_for(m: &Market, books: &HashMap<U256, OrderBookSummaryResponse>) -> Book {
    let (yb, nb) = (books.get(&m.yes_token), books.get(&m.no_token));
    let reference = yb.or(nb);
    Book {
        yes_token_id: m.yes_token,
        no_token_id: m.no_token,
        yes_bid: yb.and_then(|b| best(&b.bids, f64::max)),
        yes_ask: yb.and_then(|b| best(&b.asks, f64::min)),
        no_ask: nb.and_then(|b| best(&b.asks, f64::min)),
        tick_size: reference.and_then(|b| b.tick_size.as_decimal().to_f64()).unwrap_or(m.tick_size),
        min_order_size: reference.and_then(|b| b.min_order_size.to_f64()).unwrap_or(m.min_order_size),
    }
}

async fn order_books(
    clob: &clob::Client,
    tokens: impl Iterator<Item = U256>,
) -> Result<HashMap<U256, OrderBookSummaryResponse>> {
    let requests: Vec<_> = tokens.map(|t| OrderBookSummaryRequest::builder().token_id(t).build()).collect();
    let books = clob.order_books(&requests).await?;
    Ok(books.into_iter().map(|b| (b.asset_id, b)).collect())
}

/// Books for many markets in one request; falls back to per-market requests if the batch fails.
async fn fetch_books(clob: &clob::Client, markets: &[Market]) -> Vec<Option<Book>> {
    if markets.is_empty() {
        return Vec::new();
    }
    match order_books(clob, markets.iter().flat_map(|m| [m.yes_token, m.no_token])).await {
        Ok(books) => markets.iter().map(|m| Some(book_for(m, &books))).collect(),
        Err(e) => {
            tracing::warn!("batch book fetch failed ({e}); fetching one by one");
            let mut out = Vec::with_capacity(markets.len());
            for m in markets {
                match order_books(clob, [m.yes_token, m.no_token].into_iter()).await {
                    Ok(books) => out.push(Some(book_for(m, &books))),
                    Err(e) => {
                        tracing::warn!("book fetch failed for {}: {e}", m.slug);
                        out.push(None);
                    }
                }
            }
            out
        }
    }
}

/// Walk the most-traded open markets, keep those passing filters with a live book, and return
/// the `limit` with the best [`Candidate::opportunity`]. All `pages` are walked so the ranking
/// sees every candidate; the books come in one request per page.
pub async fn scan(
    gamma: &gamma::Client,
    clob: &clob::Client,
    s: &Settings,
    limit: usize,
    pages: u32,
) -> Result<Vec<Candidate>> {
    let mut out = Vec::new();
    // Server-side bound on the end date: long-dated markets would only be filtered out below.
    let end_date_max = Utc::now() + Duration::days(s.max_days_to_resolution + 1);
    for page in 0..i32::try_from(pages).unwrap_or(i32::MAX) {
        let request = MarketsRequest::builder()
            .limit(PAGE_SIZE)
            .offset(page * PAGE_SIZE)
            .order("volume24hr".to_owned())
            .ascending(false)
            .closed(false)
            .maybe_liquidity_num_min(Decimal::try_from(s.min_liquidity_usd).ok())
            .maybe_volume_num_min(Decimal::try_from(s.min_volume_usd).ok())
            .end_date_max(end_date_max)
            .include_tag(true)
            .build();
        let listed = gamma.markets(&request).await.context("listing markets")?;
        let passing: Vec<Market> = listed
            .iter()
            .filter_map(|m| {
                passes_static_filters(m, s)
                    .inspect_err(|why| tracing::debug!("skip {}: {why}", m.slug.as_deref().unwrap_or("?")))
                    .ok()
            })
            .collect();
        let books = fetch_books(clob, &passing).await;
        for (market, book) in passing.into_iter().zip(books) {
            let Some(book) = book else { continue };
            if let Some(spread) = book.spread().filter(|sp| *sp > s.max_spread) {
                tracing::debug!("skip {}: live spread {spread:.2}", market.slug);
                continue;
            }
            out.push(Candidate { market, book });
        }
        if listed.len() < PAGE_SIZE as usize {
            break;
        }
    }
    // Stable: equal scores keep the volume order.
    out.sort_by(|a, b| b.opportunity(s).total_cmp(&a.opportunity(s)));
    out.truncate(limit);
    Ok(out)
}

/// Fetch and store the outcomes of every decided or ordered market without one yet.
/// Returns how many were new.
pub async fn update_resolutions(gamma: &gamma::Client, store: &Store) -> Result<usize> {
    let pending = store.unresolved_condition_ids()?;
    let resolved = fetch_resolutions(gamma, &pending).await?;
    for r in &resolved {
        store.put_resolution(r)?;
    }
    Ok(resolved.len())
}

/// Resolutions of the given markets that have closed with a final price. Markets still open
/// or awaiting their outcome are left out. One request per 50 markets.
pub async fn fetch_resolutions(gamma: &gamma::Client, condition_ids: &[String]) -> Result<Vec<Resolution>> {
    let mut out = Vec::new();
    for chunk in condition_ids.chunks(PAGE_SIZE as usize) {
        let ids = chunk.iter().filter_map(|id| id.parse().ok()).collect();
        let request = MarketsRequest::builder().condition_ids(ids).closed(true).limit(PAGE_SIZE).build();
        let markets = gamma.markets(&request).await.context("fetching resolved markets")?;
        out.extend(markets.iter().filter_map(resolution));
    }
    Ok(out)
}

/// A closed market's outcome: a YES price of exactly 0 or 1, or any price once UMA marks the
/// market resolved (50-50 splits). A closed market at 0.9995 is still waiting for its outcome.
fn resolution(m: &gamma::types::response::Market) -> Option<Resolution> {
    let condition_id = m.condition_id?.to_string();
    let yes_price = m.outcome_prices.as_deref()?.first()?.to_f64()?;
    let settled = yes_price == 0.0 || yes_price == 1.0 || m.uma_resolution_status.as_deref() == Some("resolved");
    (m.closed == Some(true) && settled).then(|| Resolution {
        condition_id,
        slug: m.slug.clone().unwrap_or_default(),
        yes_price,
        resolved_at: m.closed_time.clone(),
    })
}

/// Market slug from a slug or a polymarket.com URL (`/event/<event>[/<market>]`, `/market/<slug>`).
enum MarketRef {
    Market(String),
    Event(String),
}

fn parse_ref(reference: &str) -> Result<MarketRef> {
    if !reference.starts_with("http") {
        return Ok(MarketRef::Market(reference.trim().to_owned()));
    }
    let url = reqwest::Url::parse(reference).with_context(|| format!("invalid URL {reference}"))?;
    let segments: Vec<&str> = url.path_segments().into_iter().flatten().filter(|s| !s.is_empty()).collect();
    match segments.as_slice() {
        [.., "event", _, market] | [.., "market", market] => Ok(MarketRef::Market((*market).to_owned())),
        [.., "event", event] => Ok(MarketRef::Event((*event).to_owned())),
        _ => bail!("cannot find a market slug in {reference}"),
    }
}

/// Load one market by slug or polymarket.com URL, bypassing the liquidity filters.
pub async fn load_candidate(gamma: &gamma::Client, clob: &clob::Client, reference: &str) -> Result<Candidate> {
    let raw = match parse_ref(reference)? {
        MarketRef::Market(slug) => gamma
            .market_by_slug(&MarketBySlugRequest::builder().slug(slug.as_str()).build())
            .await
            .with_context(|| format!("market {slug} not found"))?,
        MarketRef::Event(slug) => {
            let event = gamma
                .event_by_slug(&EventBySlugRequest::builder().slug(slug.as_str()).build())
                .await
                .with_context(|| format!("event {slug} not found"))?;
            match event.markets.unwrap_or_default().as_slice() {
                [only] => only.clone(),
                many => {
                    let slugs: Vec<_> = many.iter().filter_map(|m| m.slug.as_deref()).take(10).collect();
                    let more = if many.len() > slugs.len() { ", …" } else { "" };
                    bail!("event {slug} has {} markets, pass one of: {}{more}", many.len(), slugs.join(", "))
                }
            }
        }
    };
    let market = Market::try_from(&raw)
        .map_err(|why| anyhow!("{} is not a binary market: {why}", raw.slug.unwrap_or_default()))?;
    let book = fetch_books(clob, std::slice::from_ref(&market))
        .await
        .pop()
        .flatten()
        .ok_or_else(|| anyhow!("no order book for {}", market.slug))?;
    Ok(Candidate { market, book })
}

/// Compact state for Jev: numbers pre-computed (Jev is bad at date math), nothing the question
/// does not need. A research brief, when available, goes in as `evidence`.
pub fn build_state(c: &Candidate, s: &Settings, brief: Option<&Brief>) -> Value {
    let m = &c.market;
    let mut state = Map::new();
    state.insert("question".into(), json!(m.question));
    state.insert("description".into(), json!(truncate_words(&m.description, s.description_max_chars)));
    state.insert("today".into(), json!(today()));
    state.insert("days_until_resolution".into(), json!(m.days_to_resolution()));
    if let Some(src) = &m.resolution_source {
        state.insert("resolution_source".into(), json!(src));
    }
    if s.jev_sees_market_price
        && let Some(mid) = c.book.midpoint()
    {
        state.insert("market_implied_probability_yes".into(), json!((mid * 100.0).round() / 100.0));
    }
    if let Some(b) = brief {
        state.insert("evidence".into(), b.to_state(s.research_max_chars));
    }
    Value::Object(state)
}

pub fn today() -> String {
    Utc::now().date_naive().to_string()
}

fn truncate_words(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        None => text.to_owned(),
        Some((cut, _)) => {
            let head = &text[..cut];
            format!("{} …", head.rsplit_once(' ').map_or(head, |(h, _)| h))
        }
    }
}

/// A market resolving in `days` with a 2-cent YES spread and a matching NO ask.
#[cfg(test)]
pub fn test_candidate(yes_ask: f64, days: i64, liquidity: f64) -> Candidate {
    let market = Market {
        slug: "m".into(),
        question: "Q?".into(),
        description: String::new(),
        condition_id: String::new(),
        end_date: Some(Utc::now() + Duration::days(days) + Duration::hours(1)),
        resolution_source: None,
        yes_token: U256::from(1),
        no_token: U256::from(2),
        liquidity,
        volume: 0.0,
        tick_size: 0.01,
        min_order_size: 5.0,
    };
    let book = Book {
        yes_token_id: market.yes_token,
        no_token_id: market.no_token,
        yes_bid: Some(yes_ask - 0.02),
        yes_ask: Some(yes_ask),
        no_ask: Some(1.02 - yes_ask),
        tick_size: 0.01,
        min_order_size: 5.0,
    };
    Candidate { market, book }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[test]
    fn opportunity_prefers_tradable_soon_and_thin() {
        let s = Settings::default();
        let score = |yes_ask, days, liquidity| test_candidate(yes_ask, days, liquidity).opportunity(&s);
        assert_eq!(score(0.96, 3, 10_000.0), 0.0, "both asks outside the trade band");
        assert!(score(0.5, 3, 20_000.0) > score(0.5, 40, 20_000.0), "sooner resolution first");
        assert!(score(0.5, 10, 20_000.0) > score(0.5, 10, 2_000_000.0), "thin book first");
        assert!(score(0.85, 50, 2_000_000.0) > 0.0);
    }

    #[test]
    fn market_price_in_state_is_switchable() {
        let c = test_candidate(0.5, 10, 20_000.0);
        let seen = build_state(&c, &Settings::default(), None);
        assert_eq!(seen["market_implied_probability_yes"], 0.49);
        let blind = build_state(&c, &Settings { jev_sees_market_price: false, ..Settings::default() }, None);
        assert!(blind.get("market_implied_probability_yes").is_none());
    }

    #[test]
    fn excluded_tags_are_skipped() {
        let m: gamma::types::response::Market = serde_json::from_value(json!({
            "id": "1", "active": true, "acceptingOrders": true, "enableOrderBook": true,
            "tags": [{"id": "1", "label": "Crypto"}, {"id": "2", "label": "crypto prices"}]
        }))
        .unwrap();
        let why = passes_static_filters(&m, &Settings::default()).unwrap_err();
        assert_eq!(why, "tagged crypto prices");
        let open = Settings { exclude_tags: Vec::new(), ..Settings::default() };
        assert!(!passes_static_filters(&m, &open).unwrap_err().starts_with("tagged"));
    }

    #[tokio::test]
    async fn fetches_only_settled_resolutions() {
        let server = MockServer::start().await;
        let id = |n: u8| format!("0x{}", format!("{n:02x}").repeat(32));
        let market = |n: u8, prices: &str, extra: Value| {
            let mut m = json!({"id": n.to_string(), "conditionId": id(n), "slug": format!("m{n}"), "closed": true,
                               "outcomes": "[\"Yes\", \"No\"]", "outcomePrices": prices,
                               "closedTime": "2026-09-20 12:00:00+00"});
            m.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            m
        };
        let body = json!([
            market(1, "[\"1\", \"0\"]", json!({})),
            market(2, "[\"0.9995\", \"0.0005\"]", json!({})),
            market(3, "[\"0.5\", \"0.5\"]", json!({"umaResolutionStatus": "resolved"})),
            market(4, "[\"0\", \"1\"]", json!({"closed": false})),
        ]);
        Mock::given(method("GET"))
            .and(path("/markets"))
            .and(query_param("closed", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;
        let gamma = gamma::Client::new(&server.uri()).unwrap();
        let ids: Vec<String> = (1..=4).map(id).collect();
        let got = fetch_resolutions(&gamma, &ids).await.unwrap();

        let prices: Vec<_> = got.iter().map(|r| (r.slug.as_str(), r.yes_price)).collect();
        assert_eq!(prices, [("m1", 1.0), ("m3", 0.5)]);
        assert_eq!(got[0].condition_id, id(1));
        assert_eq!(got[0].resolved_at.as_deref(), Some("2026-09-20 12:00:00+00"));
        let query = server.received_requests().await.unwrap()[0].url.query().unwrap_or_default().to_owned();
        assert_eq!(query.matches("condition_ids=").count(), 4, "{query}");
    }

    #[test]
    fn parses_market_refs() {
        let slug = |r: &str| match parse_ref(r).unwrap() {
            MarketRef::Market(s) => format!("market:{s}"),
            MarketRef::Event(s) => format!("event:{s}"),
        };
        assert_eq!(slug("will-x-happen"), "market:will-x-happen");
        assert_eq!(slug("https://polymarket.com/event/fed-sept/fed-cuts-50bps?tid=1"), "market:fed-cuts-50bps");
        assert_eq!(slug("https://polymarket.com/event/fed-sept"), "event:fed-sept");
        assert_eq!(slug("https://polymarket.com/market/fed-cuts-50bps"), "market:fed-cuts-50bps");
        assert!(parse_ref("https://polymarket.com/").is_err());
    }

    #[test]
    fn truncates_on_word_boundary() {
        assert_eq!(truncate_words("short", 10), "short");
        assert_eq!(truncate_words("one two three", 9), "one two …");
    }
}
