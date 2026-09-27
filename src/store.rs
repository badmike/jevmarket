//! SQLite log of every Jev decision, research brief and order, for later calibration analysis.
//! The schema matches the Python jevymarket database, so an existing file can be reused.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use rusqlite::{Connection, OptionalExtension as _, params};
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
    cost REAL
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
    response_json TEXT
);
CREATE INDEX IF NOT EXISTS idx_decisions_slug ON decisions(slug);
CREATE INDEX IF NOT EXISTS idx_orders_condition ON orders(condition_id);
";

/// Orders in these states never reached the book.
const DEAD_STATUSES: &str = "('failed','rejected')";

#[derive(Debug)]
pub struct DecisionRow<'a> {
    pub slug: &'a str,
    pub condition_id: &'a str,
    pub question: &'a str,
    pub state: &'a Value,
    pub p_yes: f64,
    pub answerable: f64,
    pub clarity: u8,
    pub yes_ask: Option<f64>,
    pub no_ask: Option<f64>,
    pub midpoint: Option<f64>,
    pub edge: Option<f64>,
    pub action: &'a str,
    pub reason: &'a str,
    pub jev_model: Option<&'a str>,
    pub jev_cost: f64,
    pub research_cost: Option<f64>,
    pub raw: &'a Value,
}

#[derive(Debug, Default)]
pub struct OrderRow<'a> {
    pub slug: &'a str,
    pub condition_id: &'a str,
    pub token_id: &'a str,
    pub outcome: &'a str,
    pub price: f64,
    pub size: f64,
    pub usd: f64,
    pub order_id: Option<&'a str>,
    pub status: &'a str,
    pub dry_run: bool,
    pub response: Option<&'a Value>,
}

#[derive(Debug)]
pub struct Bucket {
    /// `floor(p_yes * 10)`.
    pub bucket: i64,
    pub n: i64,
    pub avg_p: f64,
    pub avg_market: f64,
}

#[derive(Debug)]
pub struct Stats {
    pub decisions: i64,
    pub trade_signals: i64,
    pub jev_cost_usd: f64,
    pub briefs: i64,
    pub research_cost_usd: f64,
    pub live_orders: i64,
    pub live_usd: f64,
    pub buckets: Vec<Bucket>,
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
        Ok(Self { conn })
    }

    pub fn log_decision(&self, r: &DecisionRow<'_>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO decisions (ts, slug, condition_id, question, state_json, p_yes, answerable, clarity,
                yes_ask, no_ask, midpoint, edge, action, reason, jev_model, jev_cost, research_cost, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
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
            ],
        )?;
        Ok(())
    }

    pub fn log_order(&self, r: &OrderRow<'_>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO orders (ts, slug, condition_id, token_id, outcome, side, price, size, usd, order_id,
                status, dry_run, response_json)
             VALUES (?1, ?2, ?3, ?4, ?5, 'BUY', ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
            ],
        )?;
        Ok(())
    }

    /// Newest brief for `slug` younger than `max_age_s`, returned with cost zeroed (already paid).
    pub fn get_brief(&self, slug: &str, max_age_s: f64) -> Result<Option<Brief>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT brief_json FROM research WHERE slug = ?1 AND ts >= ?2 ORDER BY ts DESC LIMIT 1",
                params![slug, now() - max_age_s],
                |row| row.get(0),
            )
            .optional()?;
        let Some(json) = json else { return Ok(None) };
        let brief: Brief = serde_json::from_str(&json).context("corrupt cached brief")?;
        Ok(Some(Brief { cost: 0.0, ..brief }))
    }

    pub fn put_brief(&self, slug: &str, brief: &Brief) -> Result<()> {
        self.conn.execute(
            "INSERT INTO research (ts, slug, model, brief_json, cost) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now(), slug, brief.model, serde_json::to_string(brief)?, brief.cost],
        )?;
        Ok(())
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
                "SELECT COUNT(*), COALESCE(SUM(CASE WHEN status NOT IN {DEAD_STATUSES} THEN usd END), 0)
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
        Ok(Stats { decisions, trade_signals, jev_cost_usd, briefs, research_cost_usd, live_orders, live_usd, buckets })
    }
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
    fn brief_cache_ttl() {
        let (_dir, st) = store();
        assert!(st.get_brief("slug", 3600.0).unwrap().is_none());
        let brief =
            Brief { summary: "s".into(), sources: vec!["u".into()], model: "m".into(), cost: 0.01, ..Brief::default() };
        st.put_brief("slug", &brief).unwrap();
        let hit = st.get_brief("slug", 3600.0).unwrap().unwrap();
        assert_eq!((hit.summary.as_str(), hit.sources.len(), hit.cost), ("s", 1, 0.0));

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
            state: &state,
            p_yes: 0.6,
            answerable: 0.9,
            clarity: 3,
            yes_ask: Some(0.51),
            no_ask: Some(0.5),
            midpoint: Some(0.5),
            edge: Some(0.09),
            action: "trade",
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
}
