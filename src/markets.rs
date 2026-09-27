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

/// Walk the most-traded open markets and keep those passing filters with a live book.
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
            if out.len() >= limit {
                return Ok(out);
            }
        }
        if listed.len() < PAGE_SIZE as usize {
            break;
        }
    }
    Ok(out)
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
    if let Some(mid) = c.book.midpoint() {
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

#[cfg(test)]
mod tests {
    use super::*;

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
