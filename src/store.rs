//! SQLite log of every Jev decision, research brief, order and market resolution, for
//! calibration and PnL analysis. The schema extends the Python jevymarket database additively,
//! so an existing file can be reused.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use rusqlite::{Connection, OptionalExtension as _, params};
use serde::Serialize;
use serde_json::Value;

use crate::research::Brief;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS decisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts REAL NOT NULL,
    slug TEXT NOT NULL,
    condition_id TEXT,
    question TEXT,
    state_json TEXT,
    p_yes REAL,
    answerable REAL,
    clarity INTEGER,
    yes_ask REAL,
    no_ask REAL,
    midpoint REAL,
    edge REAL,
    action TEXT,
    reason TEXT,
    -- Why a skip was a skip, see `signal::SkipCode`; NULL for trades.
    skip_code TEXT,
    jev_model TEXT,
    jev_cost REAL,
    research_cost REAL,
    raw_json TEXT
);
CREATE TABLE IF NOT EXISTS research (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts REAL NOT NULL,
    slug TEXT NOT NULL,
    model TEXT,
    brief_json TEXT NOT NULL,
    cost REAL,
    midpoint REAL
);
CREATE INDEX IF NOT EXISTS idx_research_slug ON research(slug, ts);
CREATE TABLE IF NOT EXISTS orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts REAL NOT NULL,
    slug TEXT NOT NULL,
    condition_id TEXT,
    token_id TEXT,
    outcome TEXT,
    side TEXT,
    price REAL,
    size REAL,
    usd REAL,
    order_id TEXT,
    status TEXT,
    dry_run INTEGER,
    response_json TEXT,
    -- `bot` or `manual` (placed from the console); NULL on rows from before the column.
    source TEXT,
    -- The market question when the order went out; NULL on rows from before the column.
    question TEXT
);
CREATE TABLE IF NOT EXISTS resolutions (
    condition_id TEXT PRIMARY KEY,
    slug TEXT NOT NULL,
    yes_price REAL NOT NULL,
    resolved_at TEXT,
    ts REAL NOT NULL
);
-- Markets a person asked the price watch to keep an eye on.
CREATE TABLE IF NOT EXISTS watchlist (
    slug TEXT PRIMARY KEY,
    condition_id TEXT NOT NULL,
    question TEXT,
    ts REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_decisions_slug ON decisions(slug);
CREATE INDEX IF NOT EXISTS idx_orders_condition ON orders(condition_id);
";

/// Columns added after a table was first shipped. `CREATE TABLE IF NOT EXISTS` leaves older
/// files as they are, so these are added on open when missing.
const ADDED_COLUMNS: [(&str, &str, &str); 5] = [
    ("research", "midpoint", "REAL"),
    ("orders", "source", "TEXT"),
    ("decisions", "skip_code", "TEXT"),
    ("orders", "question", "TEXT"),
    ("decisions", "image", "TEXT"),
];

/// Skip codes for decisions logged before `skip_code` existed, read off the reason text.
const BACKFILL_SKIP_CODES: &str = "
UPDATE decisions SET skip_code = CASE
    WHEN reason LIKE 'clarity %' THEN 'unclear'
    WHEN reason LIKE 'answerable %' THEN 'unanswerable'
    WHEN reason LIKE 'no asks%' THEN 'no_asks'
    WHEN reason LIKE 'outside trade band%' THEN 'outside_band'
    WHEN reason LIKE 'best edge%' THEN 'small_edge'
    WHEN reason LIKE '%suspicious_edge%' THEN 'suspicious_edge'
    WHEN reason LIKE 'kelly sizing%' THEN 'zero_stake'
    WHEN reason LIKE 'min order size%' THEN 'min_order_too_big'
    WHEN reason LIKE 'cached brief could not%' THEN 'stale_brief'
END
WHERE action = 'skip' AND skip_code IS NULL";

/// Orders in these states never reached the book.
const DEAD_STATUSES: &str = "('failed','rejected')";

/// `-1` for a SELL row of `orders o`, `1` for a BUY: proceeds and shares sold count against the stake
/// and the payout.
const SIGN: &str = "(CASE o.side WHEN 'SELL' THEN -1 ELSE 1 END)";

#[derive(Debug)]
pub struct DecisionRow<'a> {
    pub slug: &'a str,
    pub condition_id: &'a str,
    pub question: &'a str,
    /// The market's thumbnail URL.
    pub image: Option<&'a str>,
    pub state: &'a Value,
    /// `None` when only the clarity pre-screen ran.
    pub p_yes: Option<f64>,
    pub answerable: Option<f64>,
    pub clarity: u8,
    pub yes_ask: Option<f64>,
    pub no_ask: Option<f64>,
    pub midpoint: Option<f64>,
    pub edge: Option<f64>,
    pub action: &'a str,
    pub skip_code: Option<&'a str>,
    pub reason: &'a str,
    pub jev_model: Option<&'a str>,
    pub jev_cost: f64,
    pub research_cost: Option<f64>,
    pub raw: &'a Value,
}

#[derive(Debug, Default)]
pub struct OrderRow<'a> {
    pub slug: &'a str,
    pub question: &'a str,
    pub condition_id: &'a str,
    pub token_id: &'a str,
    pub outcome: &'a str,
    /// A SELL of held shares; a BUY otherwise.
    pub sell: bool,
    pub price: f64,
    pub size: f64,
    pub usd: f64,
    pub order_id: Option<&'a str>,
    pub status: &'a str,
    pub dry_run: bool,
    pub response: Option<&'a Value>,
    /// Placed from the console rather than by the signal. Stored in `source`.
    pub manual: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bucket {
    /// `floor(p_yes * 10)`.
    pub bucket: i64,
    pub n: i64,
    pub avg_p: f64,
    pub avg_market: f64,
}

/// Money spent on OpenRouter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Spend {
    pub jev_calls: u32,
    pub jev_usd: f64,
    pub briefs: u32,
    pub research_usd: f64,
}

impl std::ops::AddAssign for Spend {
    fn add_assign(&mut self, o: Self) {
        self.jev_calls += o.jev_calls;
        self.jev_usd += o.jev_usd;
        self.briefs += o.briefs;
        self.research_usd += o.research_usd;
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub decisions: i64,
    pub trade_signals: i64,
    pub jev_cost_usd: f64,
    pub briefs: i64,
    pub research_cost_usd: f64,
    pub live_orders: i64,
    pub live_usd: f64,
    pub buckets: Vec<Bucket>,
    pub calibration: Calibration,
    pub live_pnl: Pnl,
    pub dry_run_pnl: Pnl,
}

/// How a market resolved, from Gamma.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolution {
    pub condition_id: String,
    pub slug: String,
    /// Payout of one YES share: 1 for YES, 0 for NO, 0.5 for a 50-50 resolution.
    pub yes_price: f64,
    /// As Gamma reports it (`closedTime`).
    pub resolved_at: Option<String>,
}

/// Orders on resolved markets, each assumed filled in full at its limit price.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Pnl {
    /// Orders that reached the book (or would have, for dry runs).
    pub orders: i64,
    /// Of those, orders on markets that have resolved.
    pub resolved: i64,
    pub staked_usd: f64,
    pub payout_usd: f64,
    pub pnl_usd: f64,
}

/// Jev against the market on resolved markets, one decision per market: the latest one with a
/// Jev probability, so markets decided on every pass do not outweigh the rest.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Calibration {
    pub all: Group,
    /// With and without `market_implied_probability_yes` in the state.
    pub by_variant: Vec<Group>,
    /// Best edge at decision time: `p - ask` on the side Jev favored.
    pub by_edge: Vec<Group>,
    pub by_answerable: Vec<Group>,
    pub by_clarity: Vec<Group>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Group {
    pub label: String,
    pub n: i64,
    /// Mean payout of the side with the larger edge: the share of calls Jev got right.
    pub hit_rate: f64,
    /// Mean squared error of Jev's P(yes) against the outcome. Lower is better.
    pub brier_jev: f64,
    /// Same for the market midpoint at decision time.
    pub brier_market: f64,
}

/// One resolved decision, the input to [`calibrate`].
#[derive(Debug, Clone)]
pub struct Resolved {
    pub p_yes: f64,
    pub answerable: f64,
    pub clarity: u8,
    pub yes_ask: Option<f64>,
    pub no_ask: Option<f64>,
    pub midpoint: f64,
    /// Jev saw `market_implied_probability_yes`.
    pub saw_price: bool,
    pub yes_price: f64,
}

impl Resolved {
    /// `(bought YES, edge)` for the side with the larger edge; the midpoint stands in for a missing ask.
    fn best_side(&self) -> (bool, f64) {
        let yes = self.p_yes - self.yes_ask.unwrap_or(self.midpoint);
        let no = (1.0 - self.p_yes) - self.no_ask.unwrap_or(1.0 - self.midpoint);
        if yes >= no { (true, yes) } else { (false, no) }
    }
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        for (table, column, kind) in ADDED_COLUMNS {
            let exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
                [table, column],
                |r| r.get(0),
            )?;
            if !exists {
                conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"))?;
            }
        }
        conn.execute_batch(BACKFILL_SKIP_CODES)?;
        Ok(Self { conn })
    }

    pub fn log_decision(&self, r: &DecisionRow<'_>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO decisions (ts, slug, condition_id, question, state_json, p_yes, answerable, clarity,
                yes_ask, no_ask, midpoint, edge, action, reason, jev_model, jev_cost, research_cost, raw_json,
                skip_code, image)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
            params![
                now(),
                r.slug,
                r.condition_id,
                r.question,
                r.state.to_string(),
                r.p_yes,
                r.answerable,
                r.clarity,
                r.yes_ask,
                r.no_ask,
                r.midpoint,
                r.edge,
                r.action,
                r.reason,
                r.jev_model,
                r.jev_cost,
                r.research_cost,
                r.raw.to_string(),
                r.skip_code,
                r.image,
            ],
        )?;
        Ok(())
    }

    pub fn log_order(&self, r: &OrderRow<'_>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO orders (ts, slug, condition_id, token_id, outcome, side, price, size, usd, order_id,
                status, dry_run, response_json, source, question)
             VALUES (?1, ?2, ?3, ?4, ?5, ?15, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                now(),
                r.slug,
                r.condition_id,
                r.token_id,
                r.outcome,
                r.price,
                r.size,
                r.usd,
                r.order_id,
                r.status,
                r.dry_run,
                r.response.map(Value::to_string),
                if r.manual { "manual" } else { "bot" },
                Some(r.question).filter(|q| !q.is_empty()),
                if r.sell { "SELL" } else { "BUY" },
            ],
        )?;
        Ok(())
    }

    /// Newest brief for `slug` younger than `max_age_s` with the market midpoint when it was
    /// written, returned with cost zeroed (already paid).
    pub fn get_brief(&self, slug: &str, max_age_s: f64) -> Result<Option<(Brief, Option<f64>)>> {
        let row: Option<(String, Option<f64>)> = self
            .conn
            .query_row(
                "SELECT brief_json, midpoint FROM research WHERE slug = ?1 AND ts >= ?2 ORDER BY ts DESC LIMIT 1",
                params![slug, now() - max_age_s],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((json, midpoint)) = row else { return Ok(None) };
        let brief: Brief = serde_json::from_str(&json).context("corrupt cached brief")?;
        Ok(Some((Brief { cost: 0.0, ..brief }, midpoint)))
    }

    pub fn put_brief(&self, slug: &str, brief: &Brief, midpoint: Option<f64>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO research (ts, slug, model, brief_json, cost, midpoint) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now(), slug, brief.model, serde_json::to_string(brief)?, brief.cost, midpoint],
        )?;
        Ok(())
    }

    /// Condition ids of decided or ordered markets without a stored resolution.
    pub fn unresolved_condition_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT condition_id FROM decisions UNION SELECT condition_id FROM orders
             EXCEPT SELECT condition_id FROM resolutions",
        )?;
        let ids = stmt.query_map([], |r| r.get::<_, Option<String>>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ids.into_iter().flatten().filter(|id| !id.is_empty()).collect())
    }

    pub fn put_resolution(&self, r: &Resolution) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO resolutions (condition_id, slug, yes_price, resolved_at, ts)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![r.condition_id, r.slug, r.yes_price, r.resolved_at, now()],
        )?;
        Ok(())
    }

    /// Jev's calibration on resolved markets, see [`Calibration`].
    pub fn calibration(&self) -> Result<Calibration> {
        let mut stmt = self.conn.prepare(
            "SELECT d.p_yes, d.answerable, d.clarity, d.yes_ask, d.no_ask, d.midpoint, d.saw_price, r.yes_price
             FROM (SELECT *, json_extract(state_json, '$.market_implied_probability_yes') IS NOT NULL AS saw_price,
                          ROW_NUMBER() OVER (PARTITION BY condition_id ORDER BY id DESC) AS nth
                   FROM decisions WHERE p_yes IS NOT NULL AND midpoint IS NOT NULL) d
             JOIN resolutions r ON r.condition_id = d.condition_id
             WHERE d.nth = 1",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Resolved {
                    p_yes: r.get(0)?,
                    answerable: r.get::<_, Option<f64>>(1)?.unwrap_or_default(),
                    clarity: r.get::<_, Option<u8>>(2)?.unwrap_or_default(),
                    yes_ask: r.get(3)?,
                    no_ask: r.get(4)?,
                    midpoint: r.get(5)?,
                    saw_price: r.get(6)?,
                    yes_price: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(calibrate(&rows))
    }

    /// PnL of live (`dry_run = false`) or dry-run orders on resolved markets. A sale's proceeds
    /// come off the stake, and the shares it sold no longer pay out.
    pub fn pnl(&self, dry_run: bool) -> Result<Pnl> {
        let sql = format!(
            "SELECT COUNT(*), COUNT(r.yes_price), COALESCE(SUM(CASE WHEN r.yes_price IS NOT NULL THEN {SIGN} * o.usd END), 0),
                    COALESCE(SUM({SIGN} * o.size * CASE o.outcome WHEN 'YES' THEN r.yes_price ELSE 1 - r.yes_price END), 0)
             FROM orders o LEFT JOIN resolutions r ON r.condition_id = o.condition_id
             WHERE o.dry_run = ?1 AND o.status NOT IN {DEAD_STATUSES}"
        );
        Ok(self.conn.query_row(&sql, [dry_run], |r| {
            let (staked_usd, payout_usd): (f64, f64) = (r.get(2)?, r.get(3)?);
            Ok(Pnl { orders: r.get(0)?, resolved: r.get(1)?, staked_usd, payout_usd, pnl_usd: payout_usd - staked_usd })
        })?)
    }

    /// All-time OpenRouter spend as logged: every brief, and one Jev call per decision. Jev calls
    /// whose view was replaced before logging (a cached brief researched again) are not counted.
    pub fn spend(&self) -> Result<Spend> {
        let (jev_calls, jev_usd) =
            self.conn.query_row("SELECT COUNT(*), COALESCE(SUM(jev_cost), 0) FROM decisions", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let (briefs, research_usd) =
            self.conn.query_row("SELECT COUNT(*), COALESCE(SUM(cost), 0) FROM research", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        Ok(Spend { jev_calls, jev_usd, briefs, research_usd })
    }

    /// A SELL that reached the book (or would have, with `include_dry_run`) on this market.
    pub fn has_sell_for(&self, condition_id: &str, include_dry_run: bool) -> Result<bool> {
        let dry = if include_dry_run { "" } else { " AND dry_run = 0" };
        let sql = format!(
            "SELECT EXISTS(SELECT 1 FROM orders WHERE condition_id = ?1 AND side = 'SELL'
                           AND status NOT IN {DEAD_STATUSES}{dry})"
        );
        Ok(self.conn.query_row(&sql, [condition_id], |row| row.get(0))?)
    }

    /// Keep an eye on a market in the price watch, whatever its view's age.
    pub fn watch(&self, slug: &str, condition_id: &str, question: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO watchlist (slug, condition_id, question, ts) VALUES (?1, ?2, ?3, ?4)",
            params![slug, condition_id, question, now()],
        )?;
        Ok(())
    }

    /// Whether the market was on the watchlist.
    pub fn unwatch(&self, slug: &str) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM watchlist WHERE slug = ?1", [slug])? > 0)
    }

    pub fn has_order_for(&self, condition_id: &str, include_dry_run: bool) -> Result<bool> {
        let dry = if include_dry_run { "" } else { " AND dry_run = 0" };
        let sql = format!(
            "SELECT EXISTS(SELECT 1 FROM orders WHERE condition_id = ?1 AND status NOT IN {DEAD_STATUSES}{dry})"
        );
        Ok(self.conn.query_row(&sql, [condition_id], |row| row.get(0))?)
    }

    pub fn stats(&self) -> Result<Stats> {
        let (decisions, trade_signals, jev_cost_usd) = self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(action = 'trade'), 0), COALESCE(SUM(jev_cost), 0) FROM decisions",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let (briefs, research_cost_usd) =
            self.conn.query_row("SELECT COUNT(*), COALESCE(SUM(cost), 0) FROM research", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let (live_orders, live_usd) = self.conn.query_row(
            &format!(
                "SELECT COUNT(*), COALESCE(SUM(CASE WHEN status NOT IN {DEAD_STATUSES} AND side IS NOT 'SELL' THEN usd END), 0)
                 FROM orders WHERE dry_run = 0"
            ),
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut stmt = self.conn.prepare(
            "SELECT CAST(p_yes * 10 AS INT) AS b, COUNT(*), AVG(p_yes), AVG(midpoint)
             FROM decisions WHERE p_yes IS NOT NULL AND midpoint IS NOT NULL
             GROUP BY b ORDER BY b",
        )?;
        let buckets = stmt
            .query_map([], |r| Ok(Bucket { bucket: r.get(0)?, n: r.get(1)?, avg_p: r.get(2)?, avg_market: r.get(3)? }))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Stats {
            decisions,
            trade_signals,
            jev_cost_usd,
            briefs,
            research_cost_usd,
            live_orders,
            live_usd,
            buckets,
            calibration: self.calibration()?,
            live_pnl: self.pnl(false)?,
            dry_run_pnl: self.pnl(true)?,
        })
    }
}

/// Brier scores and hit rates overall and per group. Groups without decisions are left out.
pub fn calibrate(rows: &[Resolved]) -> Calibration {
    let edge = |r: &Resolved| match r.best_side().1 {
        e if e < 0.0 => 0,
        e if e < 0.04 => 1,
        e if e < 0.08 => 2,
        e if e < 0.15 => 3,
        _ => 4,
    };
    let answerable = |r: &Resolved| match r.answerable {
        a if a < 0.5 => 0,
        a if a < 0.7 => 1,
        a if a < 0.85 => 2,
        _ => 3,
    };
    Calibration {
        all: group(rows, "all", |_| true),
        by_variant: grouped(rows, &["jev sees price", "jev blind"], |r| usize::from(!r.saw_price)),
        by_edge: grouped(rows, &["< 0", "0 to 0.04", "0.04 to 0.08", "0.08 to 0.15", ">= 0.15"], edge),
        by_answerable: grouped(rows, &["< 0.50", "0.50 to 0.70", "0.70 to 0.85", ">= 0.85"], answerable),
        by_clarity: grouped(rows, &["0", "1", "2", "3", "4"], |r| usize::from(r.clarity.min(4))),
    }
}

fn grouped(rows: &[Resolved], labels: &[&str], key: impl Fn(&Resolved) -> usize) -> Vec<Group> {
    labels.iter().enumerate().map(|(i, label)| group(rows, label, |r| key(r) == i)).filter(|g| g.n > 0).collect()
}

fn group(rows: &[Resolved], label: &str, member: impl Fn(&Resolved) -> bool) -> Group {
    let (mut n, mut hits, mut jev, mut market) = (0, 0.0, 0.0, 0.0);
    for r in rows.iter().filter(|r| member(r)) {
        n += 1;
        hits += if r.best_side().0 { r.yes_price } else { 1.0 - r.yes_price };
        jev += (r.p_yes - r.yes_price).powi(2);
        market += (r.midpoint - r.yes_price).powi(2);
    }
    let mean = |sum: f64| if n == 0 { 0.0 } else { sum / n as f64 };
    Group { label: label.to_owned(), n, hit_rate: mean(hits), brier_jev: mean(jev), brier_market: mean(market) }
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let st = Store::open(&dir.path().join("t.db")).unwrap();
        (dir, st)
    }

    #[test]
    fn skip_codes_are_backfilled_from_the_reason() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let st = Store::open(&path).unwrap();
        let state = json!({});
        let row = |reason| DecisionRow {
            slug: "a",
            condition_id: "c",
            question: "Q?",
            image: None,
            state: &state,
            p_yes: None,
            answerable: None,
            clarity: 1,
            yes_ask: None,
            no_ask: None,
            midpoint: None,
            edge: None,
            action: "skip",
            skip_code: None,
            reason,
            jev_model: None,
            jev_cost: 0.0,
            research_cost: None,
            raw: &state,
        };
        st.log_decision(&row("answerable 0.21 < 0.7")).unwrap();
        st.log_decision(&row("edge +0.300 > suspicious_edge 0.25: more likely a model error")).unwrap();
        drop(st);
        let st = Store::open(&path).unwrap();
        let codes: Vec<String> = st
            .conn
            .prepare("SELECT skip_code FROM decisions ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(codes, ["unanswerable", "suspicious_edge"]);
    }

    #[test]
    fn spend_counts_briefs_and_logged_jev_calls() {
        let (_dir, st) = store();
        st.put_brief("a", &Brief { cost: 0.007, ..Brief::default() }, None).unwrap();
        st.put_brief("b", &Brief { cost: 0.003, ..Brief::default() }, None).unwrap();
        let state = json!({});
        let decision = DecisionRow {
            slug: "a",
            condition_id: "c",
            question: "Q?",
            image: None,
            state: &state,
            p_yes: None,
            answerable: None,
            clarity: 1,
            yes_ask: None,
            no_ask: None,
            midpoint: None,
            edge: None,
            action: "skip",
            skip_code: None,
            reason: "r",
            jev_model: None,
            jev_cost: 0.0001,
            research_cost: None,
            raw: &state,
        };
        st.log_decision(&decision).unwrap();
        let spend = st.spend().unwrap();
        assert_eq!((spend.jev_calls, spend.briefs), (1, 2));
        assert!((spend.research_usd - 0.01).abs() < 1e-9 && (spend.jev_usd - 0.0001).abs() < 1e-12);
    }

    #[test]
    fn brief_cache_ttl() {
        let (_dir, st) = store();
        assert!(st.get_brief("slug", 3600.0).unwrap().is_none());
        let brief =
            Brief { summary: "s".into(), sources: vec!["u".into()], model: "m".into(), cost: 0.01, ..Brief::default() };
        st.put_brief("slug", &brief, Some(0.42)).unwrap();
        let (hit, midpoint) = st.get_brief("slug", 3600.0).unwrap().unwrap();
        assert_eq!((hit.summary.as_str(), hit.sources.len(), hit.cost, midpoint), ("s", 1, 0.0, Some(0.42)));

        st.conn.execute("UPDATE research SET ts = ?1", [now() - 7200.0]).unwrap();
        assert!(st.get_brief("slug", 3600.0).unwrap().is_none(), "expired");
        let s = st.stats().unwrap();
        assert_eq!((s.briefs, s.research_cost_usd), (1, 0.01));
    }

    #[test]
    fn decision_and_order_logging() {
        let (_dir, st) = store();
        let state = json!({"q": 1});
        st.log_decision(&DecisionRow {
            slug: "a",
            condition_id: "c1",
            question: "Q?",
            image: None,
            state: &state,
            p_yes: Some(0.6),
            answerable: Some(0.9),
            clarity: 3,
            yes_ask: Some(0.51),
            no_ask: Some(0.5),
            midpoint: Some(0.5),
            edge: Some(0.09),
            action: "trade",
            skip_code: None,
            reason: "edge",
            jev_model: None,
            jev_cost: 0.0001,
            research_cost: Some(0.01),
            raw: &state,
        })
        .unwrap();
        let order = OrderRow {
            slug: "a",
            condition_id: "c1",
            token_id: "t",
            outcome: "YES",
            price: 0.5,
            size: 10.0,
            usd: 5.0,
            ..OrderRow::default()
        };
        st.log_order(&OrderRow { order_id: Some("o1"), status: "live", ..order }).unwrap();
        assert!(st.has_order_for("c1", false).unwrap());
        assert!(!st.has_order_for("c2", false).unwrap());

        let dry = OrderRow {
            slug: "b",
            condition_id: "c2",
            token_id: "t",
            outcome: "YES",
            price: 0.5,
            size: 10.0,
            usd: 5.0,
            status: "dry_run",
            dry_run: true,
            ..OrderRow::default()
        };
        st.log_order(&dry).unwrap();
        assert!(!st.has_order_for("c2", false).unwrap());
        assert!(st.has_order_for("c2", true).unwrap());

        let s = st.stats().unwrap();
        assert_eq!((s.decisions, s.trade_signals, s.live_orders, s.live_usd), (1, 1, 1, 5.0));
        assert_eq!(s.buckets.len(), 1);
        assert_eq!(s.buckets[0].bucket, 6);
    }

    #[test]
    fn adds_midpoint_column_to_old_research_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE research (id INTEGER PRIMARY KEY AUTOINCREMENT, ts REAL NOT NULL, slug TEXT NOT NULL,
                 model TEXT, brief_json TEXT NOT NULL, cost REAL);
                 INSERT INTO research (ts, slug, brief_json) VALUES (1e12, 'old', '{\"summary\":\"s\"}');",
            )
            .unwrap();
        let st = Store::open(&path).unwrap();
        let (brief, midpoint) = st.get_brief("old", 3600.0).unwrap().unwrap();
        assert_eq!((brief.summary.as_str(), midpoint), ("s", None));
        drop(st);
        Store::open(&path).unwrap();
    }

    fn decide(st: &Store, condition_id: &str, p_yes: f64, midpoint: f64, state: Value) {
        st.log_decision(&DecisionRow {
            slug: condition_id,
            condition_id,
            question: "Q?",
            image: None,
            state: &state,
            p_yes: Some(p_yes),
            answerable: Some(0.9),
            clarity: 3,
            yes_ask: Some(midpoint + 0.01),
            no_ask: Some(1.0 - midpoint + 0.01),
            midpoint: Some(midpoint),
            edge: None,
            action: "skip",
            skip_code: None,
            reason: "",
            jev_model: None,
            jev_cost: 0.0,
            research_cost: None,
            raw: &Value::Null,
        })
        .unwrap();
    }

    fn resolve(st: &Store, condition_id: &str, yes_price: f64) {
        let r =
            Resolution { condition_id: condition_id.into(), slug: condition_id.into(), yes_price, resolved_at: None };
        st.put_resolution(&r).unwrap();
    }

    #[test]
    fn calibration_uses_latest_decision_per_resolved_market() {
        let (_dir, st) = store();
        let seen = json!({"market_implied_probability_yes": 0.5});
        decide(&st, "c1", 0.2, 0.5, seen.clone());
        decide(&st, "c1", 0.9, 0.5, seen);
        decide(&st, "c2", 0.3, 0.6, json!({}));
        decide(&st, "open", 0.5, 0.5, json!({}));
        assert_eq!(st.unresolved_condition_ids().unwrap().len(), 3);
        resolve(&st, "c1", 1.0);
        resolve(&st, "c2", 0.0);
        assert_eq!(st.unresolved_condition_ids().unwrap(), ["open"]);

        let c = st.calibration().unwrap();
        assert_eq!(c.all.n, 2);
        assert_eq!(c.all.hit_rate, 1.0, "YES at 0.9 won, NO at 0.7 won");
        assert!((c.all.brier_jev - (0.01 + 0.09) / 2.0).abs() < 1e-9, "only the latest c1 decision counts");
        assert!((c.all.brier_market - (0.25 + 0.36) / 2.0).abs() < 1e-9);
        let variants: Vec<_> = c.by_variant.iter().map(|g| (g.label.as_str(), g.n)).collect();
        assert_eq!(variants, [("jev sees price", 1), ("jev blind", 1)]);
    }

    #[test]
    fn pnl_splits_live_and_dry_run() {
        let (_dir, st) = store();
        let order = |condition_id, outcome, dry_run| OrderRow {
            slug: "s",
            condition_id,
            token_id: "t",
            outcome,
            price: 0.4,
            size: 10.0,
            usd: 4.0,
            status: if dry_run { "dry_run" } else { "live" },
            dry_run,
            ..OrderRow::default()
        };
        st.log_order(&order("won", "YES", false)).unwrap();
        st.log_order(&order("lost", "NO", false)).unwrap();
        st.log_order(&order("open", "YES", false)).unwrap();
        st.log_order(&OrderRow { status: "rejected", ..order("won", "YES", false) }).unwrap();
        st.log_order(&order("won", "NO", true)).unwrap();
        resolve(&st, "won", 1.0);
        resolve(&st, "lost", 1.0);

        let live = st.pnl(false).unwrap();
        assert_eq!(
            (live.orders, live.resolved, live.staked_usd, live.payout_usd, live.pnl_usd),
            (3, 2, 8.0, 10.0, 2.0)
        );
        let dry = st.pnl(true).unwrap();
        assert_eq!((dry.orders, dry.resolved, dry.pnl_usd), (1, 1, -4.0));

        st.log_order(&OrderRow { sell: true, price: 0.7, usd: 7.0, ..order("lost", "NO", false) }).unwrap();
        assert!(st.has_sell_for("lost", false).unwrap() && !st.has_sell_for("won", true).unwrap());
        let live = st.pnl(false).unwrap();
        assert_eq!((live.staked_usd, live.payout_usd, live.pnl_usd), (1.0, 10.0, 9.0), "sold the loser at 0.70");
    }
}
