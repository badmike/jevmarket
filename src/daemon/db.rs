//! Read queries for the console. `Store` keeps its connection private and only has the queries
//! the CLI needs, so the daemon reads the same file through a second, read-only connection.

use std::path::Path;

use anyhow::{Context as _, Result};
use chrono::DateTime;
use rusqlite::{Connection, OpenFlags, OptionalExtension as _, Row};

use super::api::{
    BriefRecord, BriefSummary, OrderEvent, PnlPoint, Recommendation, RecommendationDetail, ReliabilityBin, StatsView,
    Trade,
};
use super::now;
use crate::research::Brief;
use crate::store::Store;

/// Same as the store's: orders in these states never reached the book.
const DEAD_STATUSES: &str = "('failed','rejected')";

pub struct Db {
    pub store: Store,
    conn: Connection,
}

/// Columns of [`recommendation`], after `FROM decisions d`. A market is settled once it resolved
/// or the end date in the decision's state has passed.
const RECOMMENDATION: &str = "
    SELECT d.slug, d.question, d.condition_id, d.ts, d.p_yes, d.answerable, d.clarity, d.yes_ask, d.no_ask,
           d.midpoint, d.action, d.reason, d.jev_cost, d.research_cost,
           o.outcome, o.price, o.size, o.usd, o.status, o.dry_run, d.state_json,
           EXISTS(SELECT 1 FROM resolutions WHERE condition_id = d.condition_id)
           OR COALESCE(date(json_extract(d.state_json, '$.today'),
                            json_extract(d.state_json, '$.days_until_resolution') || ' days') < date('now'), 0)
    FROM decisions d
    -- The order a pass placed right before logging this decision.
    LEFT JOIN orders o ON o.id = (
        SELECT id FROM orders WHERE condition_id = d.condition_id AND ts BETWEEN d.ts - 600 AND d.ts
            AND source IS NOT 'manual'
        ORDER BY ts DESC LIMIT 1)";

/// Columns of [`brief_record`], after `FROM research r`. A brief is settled once its market
/// resolved or, going by the latest decision's state, its end date has passed.
const BRIEF: &str = "
    SELECT r.id, r.ts, r.slug, r.brief_json, r.cost, r.midpoint,
           (SELECT question FROM decisions WHERE slug = r.slug ORDER BY id DESC LIMIT 1),
           (SELECT midpoint FROM decisions WHERE slug = r.slug ORDER BY id DESC LIMIT 1),
           EXISTS(SELECT 1 FROM resolutions WHERE slug = r.slug)
           OR COALESCE((SELECT date(json_extract(state_json, '$.today'),
                                    json_extract(state_json, '$.days_until_resolution') || ' days') < date('now')
                        FROM decisions WHERE slug = r.slug ORDER BY id DESC LIMIT 1), 0)
    FROM research r";

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        // Creates the file and migrates the schema before the read-only handle opens it.
        let store = Store::open(path)?;
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(path, flags).with_context(|| format!("opening {}", path.display()))?;
        Ok(Self { store, conn })
    }

    /// The latest decision per market, newest first.
    pub fn recommendations(&self, limit: u32) -> Result<Vec<Recommendation>> {
        let sql = format!(
            "{RECOMMENDATION} WHERE d.id IN (SELECT MAX(id) FROM decisions GROUP BY slug) ORDER BY d.ts DESC LIMIT ?1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([limit], recommendation)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn recommendation(&self, slug: &str) -> Result<Option<Recommendation>> {
        Ok(self.detail_row(slug)?.map(|(r, _)| r))
    }

    pub fn recommendation_detail(&self, slug: &str, ttl_s: f64) -> Result<Option<RecommendationDetail>> {
        let Some((recommendation, state)) = self.detail_row(slug)? else { return Ok(None) };
        let state = serde_json::from_str(&state.unwrap_or_default()).unwrap_or_default();
        let brief = self.brief_where("r.slug = ?1", slug, ttl_s)?;
        Ok(Some(RecommendationDetail { recommendation, state, brief }))
    }

    fn detail_row(&self, slug: &str) -> Result<Option<(Recommendation, Option<String>)>> {
        let sql = format!("{RECOMMENDATION} WHERE d.slug = ?1 ORDER BY d.id DESC LIMIT 1");
        let row = self.conn.query_row(&sql, [slug], |r| Ok((recommendation(r)?, r.get(20)?))).optional()?;
        Ok(row)
    }

    /// The latest brief per market, newest first.
    pub fn briefs(&self, limit: u32, ttl_s: f64) -> Result<Vec<BriefSummary>> {
        let sql =
            format!("{BRIEF} WHERE r.id IN (SELECT MAX(id) FROM research GROUP BY slug) ORDER BY r.ts DESC LIMIT ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([limit], |r| brief_record(r, ttl_s))?;
        Ok(rows.map(|r| r.map(|b| summary(&b))).collect::<rusqlite::Result<_>>()?)
    }

    pub fn brief(&self, id: i64, ttl_s: f64) -> Result<Option<BriefRecord>> {
        self.brief_where("r.id = ?1", id, ttl_s)
    }

    /// The newest brief for `slug`, as a list entry.
    pub fn latest_brief(&self, slug: &str, ttl_s: f64) -> Result<Option<BriefSummary>> {
        Ok(self.brief_where("r.slug = ?1", slug, ttl_s)?.as_ref().map(summary))
    }

    fn brief_where(&self, filter: &str, value: impl rusqlite::ToSql, ttl_s: f64) -> Result<Option<BriefRecord>> {
        let sql = format!("{BRIEF} WHERE {filter} ORDER BY r.id DESC LIMIT 1");
        Ok(self.conn.query_row(&sql, [value], |r| brief_record(r, ttl_s)).optional()?)
    }

    /// Orders the bot and the console logged, live and dry-run, newest first.
    pub fn orders(&self, limit: u32) -> Result<Vec<OrderEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT ts, slug, outcome, price, size, usd, status, dry_run, order_id, source = 'manual',
                    json_extract(response_json, '$.error_msg')
             FROM orders ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            Ok(OrderEvent {
                ts: r.get(0)?,
                slug: r.get(1)?,
                outcome: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                price: r.get::<_, Option<f64>>(3)?.unwrap_or_default(),
                size: r.get::<_, Option<f64>>(4)?.unwrap_or_default(),
                usd: r.get::<_, Option<f64>>(5)?.unwrap_or_default(),
                status: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                dry_run: r.get::<_, Option<bool>>(7)?.unwrap_or_default(),
                order_id: r.get(8)?,
                message: r.get::<_, Option<String>>(10)?.filter(|m| !m.is_empty()),
                manual: r.get::<_, Option<bool>>(9)?.unwrap_or_default(),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn stats(&self) -> Result<StatsView> {
        Ok(StatsView { stats: self.store.stats()?, reliability: self.reliability()?, pnl_series: self.pnl_series()? })
    }

    /// One decision per resolved market, the latest with a Jev probability, as `Store::calibration` counts them.
    fn reliability(&self) -> Result<Vec<ReliabilityBin>> {
        let mut stmt = self.conn.prepare(
            "SELECT MIN(CAST(d.p_yes * 10 AS INT), 9) AS b, COUNT(*), AVG(d.p_yes), AVG(d.midpoint), AVG(r.yes_price)
             FROM (SELECT condition_id, p_yes, midpoint,
                          ROW_NUMBER() OVER (PARTITION BY condition_id ORDER BY id DESC) AS nth
                   FROM decisions WHERE p_yes IS NOT NULL AND midpoint IS NOT NULL) d
             JOIN resolutions r ON r.condition_id = d.condition_id
             WHERE d.nth = 1 GROUP BY b ORDER BY b",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ReliabilityBin {
                bucket: r.get(0)?,
                n: r.get(1)?,
                avg_p: r.get(2)?,
                avg_market: r.get(3)?,
                yes_rate: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Orders on resolved markets in resolution order, each assumed filled at its limit price.
    fn pnl_series(&self) -> Result<Vec<PnlPoint>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT r.resolved_at, r.ts, o.dry_run, o.usd,
                    o.size * CASE o.outcome WHEN 'YES' THEN r.yes_price ELSE 1 - r.yes_price END
             FROM orders o JOIN resolutions r ON r.condition_id = o.condition_id
             WHERE o.status NOT IN {DEAD_STATUSES}"
        ))?;
        let mut rows = stmt
            .query_map([], |r| {
                let resolved_at: Option<String> = r.get(0)?;
                let ts = resolved_at.as_deref().and_then(parse_time).unwrap_or(r.get(1)?);
                let (usd, payout): (Option<f64>, Option<f64>) = (r.get(3)?, r.get(4)?);
                Ok((
                    ts,
                    r.get::<_, Option<bool>>(2)?.unwrap_or_default(),
                    payout.unwrap_or_default() - usd.unwrap_or_default(),
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut live, mut dry) = (0.0, 0.0);
        Ok(rows
            .into_iter()
            .map(|(ts, dry_run, pnl_usd)| {
                let total = if dry_run { &mut dry } else { &mut live };
                *total += pnl_usd;
                PnlPoint { ts, dry_run, pnl_usd, cumulative_usd: *total }
            })
            .collect())
    }
}

fn recommendation(r: &Row<'_>) -> rusqlite::Result<Recommendation> {
    let (p_yes, yes_ask, no_ask): (Option<f64>, Option<f64>, Option<f64>) = (r.get(4)?, r.get(7)?, r.get(8)?);
    let (edge, side) = best_edge(p_yes, yes_ask, no_ask).unzip();
    let trade = match r.get::<_, Option<String>>(14)? {
        Some(outcome) => Some(Trade {
            outcome,
            price: r.get::<_, Option<f64>>(15)?.unwrap_or_default(),
            size: r.get::<_, Option<f64>>(16)?.unwrap_or_default(),
            usd: r.get::<_, Option<f64>>(17)?.unwrap_or_default(),
            status: r.get(18)?,
            dry_run: r.get::<_, Option<bool>>(19)?.unwrap_or_default(),
        }),
        None => None,
    };
    Ok(Recommendation {
        slug: r.get(0)?,
        question: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
        condition_id: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
        ts: r.get(3)?,
        p_yes,
        answerable: r.get(5)?,
        clarity: r.get(6)?,
        yes_ask,
        no_ask,
        midpoint: r.get(9)?,
        edge,
        side,
        action: r.get::<_, Option<String>>(10)?.unwrap_or_default(),
        reason: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
        trade,
        jev_cost: r.get::<_, Option<f64>>(12)?.unwrap_or_default(),
        research_cost: r.get(13)?,
        settled: r.get(21)?,
    })
}

/// The side with the larger `p - ask`, before the trade band and the gates.
pub fn best_edge(p_yes: Option<f64>, yes_ask: Option<f64>, no_ask: Option<f64>) -> Option<(f64, &'static str)> {
    let p = p_yes?;
    let yes = yes_ask.map(|a| (p - a, "YES"));
    let no = no_ask.map(|a| (1.0 - p - a, "NO"));
    [yes, no].into_iter().flatten().max_by(|a, b| a.0.total_cmp(&b.0))
}

fn brief_record(r: &Row<'_>, ttl_s: f64) -> rusqlite::Result<BriefRecord> {
    let ts: f64 = r.get(1)?;
    let json: String = r.get(3)?;
    // A brief the current `Brief` cannot read is shown empty rather than failing the whole list.
    let brief: Brief = serde_json::from_str(&json).unwrap_or_default();
    Ok(BriefRecord {
        id: r.get(0)?,
        ts,
        slug: r.get(2)?,
        question: r.get(6)?,
        cost: r.get::<_, Option<f64>>(4)?.unwrap_or_default(),
        fresh: now() - ts < ttl_s,
        midpoint_then: r.get(5)?,
        midpoint_now: r.get(7)?,
        settled: r.get(8)?,
        brief,
    })
}

pub fn summary(b: &BriefRecord) -> BriefSummary {
    BriefSummary {
        id: b.id,
        ts: b.ts,
        slug: b.slug.clone(),
        question: b.question.clone(),
        as_of: b.brief.as_of.clone(),
        summary: b.brief.summary.clone(),
        model: b.brief.model.clone(),
        cost: b.cost,
        fresh: b.fresh,
        settled: b.settled,
        facts: b.brief.key_facts.len(),
        sources: b.brief.sources.len(),
    }
}

/// Gamma's `closedTime` (`2026-09-20 12:00:00+00`) or RFC 3339, as unix seconds.
fn parse_time(s: &str) -> Option<f64> {
    DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%#z")
        .or_else(|_| DateTime::parse_from_rfc3339(s))
        .ok()
        .map(|t| t.timestamp() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_edge_picks_the_larger_side() {
        assert_eq!(best_edge(Some(0.6), Some(0.5), Some(0.52)).map(|e| e.1), Some("YES"));
        let (edge, side) = best_edge(Some(0.2), Some(0.4), Some(0.62)).unwrap();
        assert_eq!(side, "NO");
        assert!((edge - 0.18).abs() < 1e-9);
        assert_eq!(best_edge(None, Some(0.5), None), None);
    }

    #[test]
    fn manual_orders_are_flagged_and_not_attached_to_decisions() {
        use crate::store::{DecisionRow, OrderRow};
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).unwrap();
        let order = OrderRow { slug: "m", condition_id: "c", outcome: "YES", status: "live", ..OrderRow::default() };
        db.store.log_order(&OrderRow { manual: true, ..order }).unwrap();
        let state = serde_json::json!({});
        db.store
            .log_decision(&DecisionRow {
                slug: "m",
                condition_id: "c",
                question: "Q?",
                state: &state,
                p_yes: Some(0.5),
                answerable: Some(0.9),
                clarity: 3,
                yes_ask: Some(0.5),
                no_ask: Some(0.5),
                midpoint: Some(0.5),
                edge: None,
                action: "skip",
                reason: "r",
                jev_model: None,
                jev_cost: 0.0,
                research_cost: None,
                raw: &state,
            })
            .unwrap();
        assert!(db.orders(10).unwrap()[0].manual);
        assert!(db.recommendation("m").unwrap().unwrap().trade.is_none(), "the skip did not place it");
    }

    #[test]
    fn briefs_of_resolved_markets_are_settled() {
        use crate::store::Resolution;
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).unwrap();
        for slug in ["open", "done"] {
            db.store.put_brief(slug, &Brief::default(), None).unwrap();
        }
        let r = Resolution { condition_id: "c".into(), slug: "done".into(), yes_price: 1.0, resolved_at: None };
        db.store.put_resolution(&r).unwrap();
        let settled = |slug| db.latest_brief(slug, 3600.0).unwrap().unwrap().settled;
        assert!(!settled("open") && settled("done"));
    }

    #[test]
    fn parses_gamma_close_times() {
        assert_eq!(parse_time("2026-09-20 12:00:00+00"), Some(1_789_905_600.0));
        assert_eq!(parse_time("2026-09-20T12:00:00Z"), Some(1_789_905_600.0));
        assert_eq!(parse_time("soon"), None);
    }
}
