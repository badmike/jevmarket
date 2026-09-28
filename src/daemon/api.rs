//! Wire types of the daemon's HTTP API and event stream.
//!
//! `web/src/api/types.ts` mirrors this file by hand. Change both together.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::research::Brief;
pub use crate::store::Spend;
use crate::store::Stats;
pub use crate::wallets::{Transfer, Wallets};

/// What the trading loop is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopState {
    /// Idle until `next_pass_at`.
    Waiting,
    Running,
    /// A pass is finishing its current market after pause or shutdown.
    Stopping,
    Paused,
}

/// One trading pass, in progress or finished.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Pass {
    pub number: u64,
    pub started_at: f64,
    pub finished_at: Option<f64>,
    pub dry_run: bool,
    /// Markets to assess after the exposure filter.
    pub candidates: usize,
    pub assessed: usize,
    pub trades: u32,
    pub spend: Spend,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Status {
    pub version: &'static str,
    /// Identifies the embedded console. A console that reconnects to a different build reloads,
    /// since its code and asset names no longer match the server's.
    pub build: String,
    pub started_at: f64,
    pub state: LoopState,
    pub loop_secs: u64,
    pub next_pass_at: Option<f64>,
    /// Effective setting: the config value, or forced on by `--dry-run`.
    pub dry_run: bool,
    pub dry_run_forced: bool,
    pub pass: Option<Pass>,
    pub last_pass: Option<Pass>,
    /// All time: what the database logged before the daemon started, plus every pass and single
    /// decision since.
    pub spend_total: Spend,
    pub last_error: Option<String>,
}

/// A buy the signal proposed, and what became of it when an order was logged.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Trade {
    pub outcome: String,
    pub price: f64,
    pub size: f64,
    pub usd: f64,
    /// Order status (`dry_run`, `live`, `matched`, `rejected`, ...), `None` if never placed.
    pub status: Option<String>,
    pub dry_run: bool,
}

/// The latest Jev view and verdict for one market.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Recommendation {
    pub slug: String,
    pub question: String,
    pub condition_id: String,
    pub ts: f64,
    /// `None` when only the clarity pre-screen ran.
    pub p_yes: Option<f64>,
    pub answerable: Option<f64>,
    pub clarity: Option<u8>,
    pub yes_ask: Option<f64>,
    pub no_ask: Option<f64>,
    pub midpoint: Option<f64>,
    /// Best `p - ask` over both sides, before the trade band and gates.
    pub edge: Option<f64>,
    pub side: Option<&'static str>,
    /// `trade`, `trade_unexecuted`, `skip`, or whatever the pipeline logs.
    pub action: String,
    /// Why a skip was a skip (`signal::SkipCode`), `None` for trades.
    pub skip_code: Option<String>,
    pub reason: String,
    pub trade: Option<Trade>,
    pub jev_cost: f64,
    pub research_cost: Option<f64>,
    /// The market resolved or its end date passed.
    pub settled: bool,
}

/// A recommendation with the state Jev saw and the brief behind it.
#[derive(Debug, Clone, Serialize)]
pub struct RecommendationDetail {
    #[serde(flatten)]
    pub recommendation: Recommendation,
    pub state: Value,
    pub brief: Option<BriefRecord>,
}

/// A stored brief.
#[derive(Debug, Clone, Serialize)]
pub struct BriefRecord {
    pub id: i64,
    pub ts: f64,
    pub slug: String,
    pub question: Option<String>,
    pub cost: f64,
    /// Inside `research_ttl_hours`, so the pipeline would reuse it.
    pub fresh: bool,
    /// Market midpoint when the brief was written, and at the latest decision.
    pub midpoint_then: Option<f64>,
    pub midpoint_now: Option<f64>,
    /// The market resolved or its end date passed: the brief is history.
    pub settled: bool,
    pub brief: Brief,
}

/// A brief in a list, without the long fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BriefSummary {
    pub id: i64,
    pub ts: f64,
    pub slug: String,
    pub question: Option<String>,
    pub as_of: String,
    pub summary: String,
    pub model: String,
    pub cost: f64,
    pub fresh: bool,
    pub settled: bool,
    pub facts: usize,
    pub sources: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Position {
    pub slug: String,
    pub title: String,
    pub outcome: String,
    pub size: f64,
    pub avg_price: f64,
    pub cur_price: f64,
    pub value_usd: f64,
    pub pnl_usd: f64,
    pub redeemable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenOrder {
    pub id: String,
    /// Condition id.
    pub market: String,
    /// The market, when the local log knows it.
    pub slug: Option<String>,
    pub title: Option<String>,
    pub side: String,
    pub outcome: String,
    pub price: f64,
    pub size: f64,
    pub matched: f64,
    pub status: String,
    pub created_at: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Exposure {
    pub positions_usd: f64,
    pub open_orders_usd: f64,
    pub total_usd: f64,
    pub cap_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Positions {
    pub wallet: Option<String>,
    pub wallet_type: &'static str,
    /// pUSD the exchange sees; `None` without a key.
    pub balance_usd: Option<f64>,
    pub positions: Vec<Position>,
    pub open_orders: Vec<OpenOrder>,
    pub exposure: Exposure,
}

/// Observed YES rate against Jev's probability, on resolved markets.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReliabilityBin {
    /// `floor(p_yes * 10)`.
    pub bucket: i64,
    pub n: i64,
    pub avg_p: f64,
    pub avg_market: f64,
    pub yes_rate: f64,
}

/// Cumulative PnL after each resolved order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PnlPoint {
    pub ts: f64,
    pub dry_run: bool,
    pub pnl_usd: f64,
    pub cumulative_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsView {
    #[serde(flatten)]
    pub stats: Stats,
    pub reliability: Vec<ReliabilityBin>,
    pub pnl_series: Vec<PnlPoint>,
}

/// Settings as the console may see them: secrets reduced to whether they are set.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigView {
    pub path: String,
    /// Effective values of every non-secret key.
    pub values: Map<String, Value>,
    pub defaults: Map<String, Value>,
    pub secrets: Vec<SecretState>,
    /// Keys an environment variable overrides; edits to them have no effect until it is unset.
    pub env_overrides: Vec<String>,
    pub dry_run_forced: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SecretState {
    pub key: String,
    pub set: bool,
}

/// What a pass, a single decision or an order did, for the activity feed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OrderEvent {
    pub ts: f64,
    pub slug: String,
    /// The market question, the slug when unknown.
    pub title: String,
    pub outcome: String,
    pub price: f64,
    pub size: f64,
    pub usd: f64,
    pub status: String,
    pub dry_run: bool,
    pub order_id: Option<String>,
    pub message: Option<String>,
    /// Placed from the console rather than by the signal.
    pub manual: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
}

/// One message on `GET /api/events`, sent as the SSE `data` field.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Status {
        status: Status,
    },
    PassStarted {
        pass: Pass,
    },
    PassFinished {
        pass: Pass,
    },
    Decision {
        recommendation: Recommendation,
    },
    Order {
        order: OrderEvent,
    },
    Brief {
        brief: BriefSummary,
    },
    PositionsChanged,
    StatsChanged,
    ConfigChanged,
    Log {
        ts: f64,
        level: LogLevel,
        message: String,
    },
    /// Events were dropped for this client: reload every snapshot.
    Resync,
}

impl Event {
    /// Kept in the recent-activity buffer that a fresh console starts from.
    pub fn is_activity(&self) -> bool {
        matches!(
            self,
            Self::PassStarted { .. }
                | Self::PassFinished { .. }
                | Self::Decision { .. }
                | Self::Order { .. }
                | Self::Brief { .. }
                | Self::Log { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn events_serialize_with_a_type_tag() {
        assert_eq!(serde_json::to_value(Event::StatsChanged).unwrap(), json!({"type": "stats_changed"}));
        let log = Event::Log { ts: 1.0, level: LogLevel::Warn, message: "m".into() };
        assert_eq!(
            serde_json::to_value(log).unwrap(),
            json!({"type": "log", "ts": 1.0, "level": "warn", "message": "m"})
        );
        let pass = Event::PassStarted { pass: Pass { number: 3, dry_run: true, ..Pass::default() } };
        let v = serde_json::to_value(pass).unwrap();
        assert_eq!((v["type"].as_str(), v["pass"]["number"].as_u64()), (Some("pass_started"), Some(3)));
        assert_eq!(v["pass"]["spend"], json!({"jev_calls": 0, "jev_usd": 0.0, "briefs": 0, "research_usd": 0.0}));
    }

    #[test]
    fn detail_flattens_the_recommendation() {
        let r = Recommendation {
            slug: "s".into(),
            question: "Q?".into(),
            condition_id: "c".into(),
            ts: 1.0,
            p_yes: Some(0.6),
            answerable: Some(0.8),
            clarity: Some(3),
            yes_ask: Some(0.5),
            no_ask: Some(0.52),
            midpoint: Some(0.49),
            edge: Some(0.1),
            side: Some("YES"),
            action: "skip".into(),
            skip_code: Some("small_edge".into()),
            reason: "r".into(),
            trade: None,
            jev_cost: 0.0,
            research_cost: None,
            settled: false,
        };
        let v =
            serde_json::to_value(RecommendationDetail { recommendation: r, state: json!({}), brief: None }).unwrap();
        assert_eq!((v["slug"].as_str(), v["side"].as_str()), (Some("s"), Some("YES")));
        assert!(v["state"].is_object() && v["brief"].is_null());
    }
}
