//! Read queries for the console. `Store` keeps its connection private and only has the queries
//! the CLI needs, so the daemon reads the same file through a second, read-only connection.

use std::path::Path;

use anyhow::{Context as _, Result};
use chrono::DateTime;
use rusqlite::{Connection, OpenFlags, OptionalExtension as _, Row, params};

use super::api::{
    BriefRecord, BriefSummary, OrderEvent, PnlPoint, Recommendation, RecommendationDetail, ReliabilityBin, StatsView,
    Trade,
};
use super::now;
use super::price_watch::View;
use crate::config::Settings;
use crate::research::Brief;
use crate::signal::JevView;
use crate::store::Store;

/// Same as the store's: orders in these states never reached the book.
const DEAD_STATUSES: &str = "('failed','rejected')";

/// Same as the store's: a SELL row counts against the stake and the payout.
const SIGN: &str = "(CASE o.side WHEN 'SELL' THEN -1 ELSE 1 END)";

pub struct Db {
    pub store: Store,
    conn: Connection,
}

/// SQL for decision `d`: its market is settled, it resolved or the end date in its state has passed.
macro_rules! settled {
    () => {
        "(EXISTS(SELECT 1 FROM resolutions WHERE condition_id = d.condition_id)
          OR COALESCE(date(json_extract(d.state_json, '$.today'),
                           json_extract(d.state_json, '$.days_until_resolution') || ' days') < date('now'), 0))"
    };
}

/// SQL for decision `d`: the day its market resolves, going by the state Jev saw.
macro_rules! end_date {
    () => {
        "date(json_extract(d.state_json, '$.today'), json_extract(d.state_json, '$.days_until_resolution') || ' days')"
    };
}

/// Columns of [`recommendation`], after `FROM decisions d`.
const RECOMMENDATION: &str = concat!(
    "
    SELECT d.slug, d.question, d.condition_id, d.ts, d.p_yes, d.answerable, d.clarity, d.yes_ask, d.no_ask,
           d.midpoint, d.action, d.reason, d.jev_cost, d.research_cost,
           o.outcome, o.price, o.size, o.usd, o.status, o.dry_run, d.state_json, ",
    settled!(),
    ", d.skip_code, o.side, ",
    end_date!(),
    ", EXISTS(SELECT 1 FROM watchlist WHERE slug = d.slug), d.image
    FROM decisions d
    -- The order a pass placed right before logging this decision.
    LEFT JOIN orders o ON o.id = (
        SELECT id FROM orders WHERE condition_id = d.condition_id AND ts BETWEEN d.ts - 600 AND d.ts
            AND source IS NOT 'manual'
        ORDER BY ts DESC LIMIT 1)"
);

/// Columns of [`view`], after `FROM decisions d`.
const VIEW: &str = "
    SELECT d.slug, d.question, d.condition_id, d.ts, d.p_yes, d.answerable, d.clarity, d.midpoint,
           d.state_json, d.jev_model, d.raw_json,
           (SELECT MAX(ts) FROM research WHERE slug = d.slug AND ts <= d.ts),
           EXISTS(SELECT 1 FROM watchlist WHERE slug = d.slug)
    FROM decisions d";

/// The price watch's views, see [`Db::watch_views`]. `?1` oldest view, `?2` min answerable,
/// `?3` min clarity, `?4` whether dry-run orders count.
const WATCH_VIEWS: &str = concat!(
    "
    WHERE d.id IN (SELECT MAX(id) FROM decisions GROUP BY slug)
      AND d.p_yes IS NOT NULL
      -- The watchlist a person keeps is watched whatever the view's age and gates.
      AND (EXISTS(SELECT 1 FROM watchlist WHERE slug = d.slug)
           OR (d.ts >= ?1 AND d.answerable >= ?2 AND d.clarity >= ?3))
      AND COALESCE(d.condition_id, '') <> ''
      -- A signal that could not be renewed waits for the next research cycle.
      AND COALESCE(d.skip_code, '') NOT IN ('stale_brief', 'stale_view')
      AND NOT ",
    settled!(),
    "
      AND NOT EXISTS(SELECT 1 FROM orders WHERE condition_id = d.condition_id
                     AND status NOT IN ('failed','rejected') AND (dry_run = 0 OR ?4))
    ORDER BY d.ts DESC"
);

/// Columns of [`brief_record`], after `FROM research r`. A brief is settled once its market
/// resolved or, going by the latest decision's state, its end date has passed.
const BRIEF: &str = "
    SELECT r.id, r.ts, r.slug, r.brief_json, r.cost, r.midpoint,
           (SELECT question FROM decisions WHERE slug = r.slug ORDER BY id DESC LIMIT 1),
           (SELECT midpoint FROM decisions WHERE slug = r.slug ORDER BY id DESC LIMIT 1),
           EXISTS(SELECT 1 FROM resolutions WHERE slug = r.slug)
           OR COALESCE((SELECT date(json_extract(state_json, '$.today'),
                                    json_extract(state_json, '$.days_until_resolution') || ' days') < date('now')
                        FROM decisions WHERE slug = r.slug ORDER BY id DESC LIMIT 1), 0),
           (SELECT image FROM decisions WHERE slug = r.slug AND image IS NOT NULL ORDER BY id DESC LIMIT 1)
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

    /// The Jev views the price watch holds against live books: each market's latest decision, when
    /// it has a probability, did not end in a failed renewal, its market is open and has no order
    /// yet (a dry-run order counts in dry runs, so the watch does not buy the same market every
    /// tick), and it passes the answerable and clarity gates and is younger than
    /// `research_ttl_hours` or is on the watchlist.
    pub fn watch_views(&self, s: &Settings) -> Result<Vec<View>> {
        let oldest = now() - s.research_ttl_hours * 3600.0;
        let mut stmt = self.conn.prepare(&format!("{VIEW} {WATCH_VIEWS}"))?;
        let rows = stmt.query_map(params![oldest, s.min_answerable, s.min_clarity, s.dry_run], view)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The latest Jev view with a probability on each of these markets, for the exits of held
    /// positions. Markets Jev never priced are left out.
    pub fn exit_views(&self, condition_ids: &[String]) -> Result<Vec<View>> {
        let sql = format!(
            "{VIEW} WHERE d.id IN (SELECT MAX(id) FROM decisions WHERE p_yes IS NOT NULL
                                   AND condition_id IN (SELECT value FROM json_each(?1)) GROUP BY condition_id)"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([serde_json::to_string(condition_ids)?], view)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Whether a SELL of this market already went out, so a held position is not sold twice.
    pub fn sold(&self, condition_id: &str, include_dry_run: bool) -> Result<bool> {
        self.store.has_sell_for(condition_id, include_dry_run)
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
                    json_extract(response_json, '$.error_msg'),
                    COALESCE(question, (SELECT question FROM decisions WHERE slug = o.slug ORDER BY id DESC LIMIT 1)),
                    COALESCE(side, 'BUY')
             FROM orders o ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            let slug: String = r.get(1)?;
            Ok(OrderEvent {
                ts: r.get(0)?,
                title: r.get::<_, Option<String>>(11)?.unwrap_or_else(|| slug.clone()),
                slug,
                side: r.get(12)?,
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

    /// `(slug, question)` of the market an exchange order belongs to, from the local log: the
    /// order itself, else any order or decision on the same condition.
    pub fn order_market(&self, order_id: &str, condition_id: &str) -> Result<Option<(String, Option<String>)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT slug, question FROM (
                     SELECT slug, question, order_id = ?1 AS exact, id FROM orders
                     WHERE order_id = ?1 OR condition_id = ?2
                     UNION ALL
                     SELECT slug, question, 0, -1 FROM decisions WHERE condition_id = ?2
                 ) ORDER BY exact DESC, question IS NULL, id DESC LIMIT 1",
                [order_id, condition_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
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
            "SELECT r.resolved_at, r.ts, o.dry_run, {SIGN} * o.usd,
                    {SIGN} * o.size * CASE o.outcome WHEN 'YES' THEN r.yes_price ELSE 1 - r.yes_price END
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
            side: r.get::<_, Option<String>>(23)?.unwrap_or_else(|| "BUY".into()),
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
        skip_code: r.get(22)?,
        reason: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
        trade,
        jev_cost: r.get::<_, Option<f64>>(12)?.unwrap_or_default(),
        research_cost: r.get(13)?,
        settled: r.get(21)?,
        end_date: r.get(24)?,
        pinned: r.get(25)?,
        image: r.get(26)?,
    })
}

/// A row of [`VIEW`].
fn view(r: &Row<'_>) -> rusqlite::Result<View> {
    let json = |text: Option<String>| text.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    Ok(View {
        slug: r.get(0)?,
        question: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
        condition_id: r.get(2)?,
        ts: r.get(3)?,
        jev: JevView {
            p_yes: r.get(4)?,
            answerable: r.get(5)?,
            clarity: r.get(6)?,
            clarity_mean: None,
            clarity_confidence: None,
            model: r.get(9)?,
            cost: 0.0,
            raw: json(r.get(10)?),
        },
        midpoint: r.get(7)?,
        state: json(r.get(8)?),
        brief_at: r.get(11)?,
        pinned: r.get(12)?,
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
        image: r.get(9)?,
        brief,
    })
}

pub fn summary(b: &BriefRecord) -> BriefSummary {
    BriefSummary {
        id: b.id,
        ts: b.ts,
        slug: b.slug.clone(),
        question: b.question.clone(),
        image: b.image.clone(),
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
                image: None,
                state: &state,
                p_yes: Some(0.5),
                answerable: Some(0.9),
                clarity: 3,
                yes_ask: Some(0.5),
                no_ask: Some(0.5),
                midpoint: Some(0.5),
                edge: None,
                action: "skip",
                skip_code: None,
                reason: "r",
                jev_model: None,
                jev_cost: 0.0,
                research_cost: None,
                raw: &state,
            })
            .unwrap();
        assert!(db.orders(10).unwrap()[0].manual);
        assert_eq!(db.orders(10).unwrap()[0].title, "Q?", "the question comes from the decision");
        let (slug, question) = db.order_market("unknown-order", "c").unwrap().unwrap();
        assert_eq!((slug.as_str(), question.as_deref()), ("m", Some("Q?")));
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
    fn watch_views_are_recent_gated_open_and_unordered() {
        use serde_json::{Value, json};

        use crate::store::{DecisionRow, OrderRow};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let db = Db::open(&path).unwrap();
        let decide = |slug: &str, p_yes: Option<f64>, answerable: f64, clarity: u8, skip_code, state: Value| {
            let row = DecisionRow {
                slug,
                condition_id: slug,
                question: "Q?",
                image: None,
                state: &state,
                p_yes,
                answerable: Some(answerable),
                clarity,
                yes_ask: Some(0.5),
                no_ask: Some(0.52),
                midpoint: Some(0.49),
                edge: None,
                action: "skip",
                skip_code,
                reason: "r",
                jev_model: None,
                jev_cost: 0.0,
                research_cost: None,
                raw: &Value::Null,
            };
            db.store.log_decision(&row).unwrap();
        };
        db.store.put_brief("watched", &Brief::default(), Some(0.49)).unwrap();
        for slug in ["watched", "prescreened", "old", "ordered", "dry-ordered"] {
            decide(slug, Some(0.6), 0.9, 3, None, json!({}));
        }
        decide("prescreened", None, 0.9, 1, Some("unclear"), json!({}));
        decide("unanswerable", Some(0.6), 0.5, 3, None, json!({}));
        decide("unclear", Some(0.6), 0.9, 1, None, json!({}));
        decide("failed", Some(0.6), 0.9, 3, Some("stale_brief"), json!({}));
        decide("ended", Some(0.6), 0.9, 3, None, json!({"today": "2026-01-01", "days_until_resolution": 3}));
        let rw = Connection::open(&path).unwrap();
        rw.execute("UPDATE decisions SET ts = ts - 7 * 3600 WHERE slug = 'old'", []).unwrap();
        let order = |condition_id, dry_run| OrderRow {
            slug: condition_id,
            condition_id,
            status: if dry_run { "dry_run" } else { "live" },
            dry_run,
            ..OrderRow::default()
        };
        db.store.log_order(&order("ordered", false)).unwrap();
        db.store.log_order(&order("dry-ordered", true)).unwrap();

        let views = |dry_run| db.watch_views(&Settings { dry_run, ..Settings::default() }).unwrap();
        let mut live: Vec<_> = views(false).into_iter().map(|v| (v.slug, v.brief_at.is_some())).collect();
        live.sort();
        assert_eq!(live, [("dry-ordered".into(), false), ("watched".into(), true)]);
        let dry: Vec<_> = views(true).into_iter().map(|v| v.slug).collect();
        assert_eq!(dry, ["watched"], "a dry-run order counts in dry runs");

        db.store.watch("old", "old", "Q?").unwrap();
        db.store.watch("unanswerable", "unanswerable", "Q?").unwrap();
        let mut pinned: Vec<_> = views(true).into_iter().filter(|v| v.pinned).map(|v| v.slug).collect();
        pinned.sort();
        assert_eq!(pinned, ["old", "unanswerable"], "the watchlist ignores age and gates");

        let held = ["ordered".to_owned(), "prescreened".to_owned(), "nowhere".to_owned()];
        let mut exits: Vec<_> = db.exit_views(&held).unwrap().into_iter().map(|v| (v.slug, v.jev.p_yes)).collect();
        exits.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(exits, [("ordered".into(), 0.6), ("prescreened".into(), 0.6)], "the latest priced view");
    }

    #[test]
    fn parses_gamma_close_times() {
        assert_eq!(parse_time("2026-09-20 12:00:00+00"), Some(1_789_905_600.0));
        assert_eq!(parse_time("2026-09-20T12:00:00Z"), Some(1_789_905_600.0));
        assert_eq!(parse_time("soon"), None);
    }
}
