//! The one path from candidate to verdict: clarity pre-screen, research (cached) -> state ->
//! Jev -> evaluate, the stale-evidence guard, plus logging.

use std::future::Future;

use anyhow::Result;
use futures::{FutureExt as _, StreamExt as _};
use polymarket_client_sdk_v2::{clob, gamma};
use serde_json::Value;

use crate::config::Settings;
use crate::executor::{Executor, Placed};
use crate::jev::JevClient;
use crate::markets::{Candidate, build_state, scan, today, update_resolutions};
use crate::openrouter::{OpenRouter, OpenRouterError};
use crate::research::{Brief, Researcher, Topic};
use crate::signal::{JevView, Verdict, ask_clarity, ask_jev, evaluate};
use crate::store::{DecisionRow, Store};

pub struct Assessment {
    pub state: Value,
    pub brief: Option<Brief>,
    /// The brief came from the cache (already paid for).
    pub cached: bool,
    pub view: JevView,
}

/// What [`Pipeline::decide`] concluded about a market.
pub enum Decided {
    /// The clarity pre-screen failed before any research was paid for. Already logged; holds the reason.
    Unclear(String),
    /// Log with [`Pipeline::log`] once the trade, if any, was attempted.
    Assessed(Box<Assessment>, Verdict),
}

/// How much research a pipeline may do.
#[derive(Debug, Clone, Copy)]
pub enum Research {
    Off,
    /// At most this many researcher calls until [`Pipeline::reset_budget`].
    Budget(u32),
    Unlimited,
}

pub struct Pipeline<'a> {
    s: &'a Settings,
    store: &'a Store,
    pub jev: JevClient,
    pub researcher: Option<Researcher>,
}

/// Whether an error means every further OpenRouter call would fail too.
pub fn is_fatal(e: &anyhow::Error) -> bool {
    e.downcast_ref::<OpenRouterError>().is_some_and(OpenRouterError::is_fatal)
}

impl<'a> Pipeline<'a> {
    pub fn new(s: &'a Settings, store: &'a Store, research: Research) -> Result<Self> {
        let api = OpenRouter::new(s.require_openrouter_key()?, &s.openrouter_base_url);
        let max_calls = match research {
            Research::Off => None,
            Research::Budget(n) => Some(Some(n)),
            Research::Unlimited => Some(None),
        };
        let researcher = max_calls.map(|max_calls| {
            Researcher::new(
                api.clone(),
                &s.research_model,
                s.research_max_results,
                max_calls,
                s.research_exclude_domains.clone(),
            )
        });
        Ok(Self { s, store, jev: JevClient::new(api, &s.jev_model), researcher })
    }

    /// `(brief, from_cache)`: the cached brief unless `fresh`, else a new one. No brief if
    /// research is disabled, over budget, or failed; only fatal OpenRouter errors (bad key, no
    /// credits) are returned as errors.
    pub async fn brief(&self, c: &Candidate, fresh: bool) -> Result<(Option<Brief>, bool)> {
        if !fresh && let Some(b) = self.cached_brief(c)? {
            return Ok((Some(b), true));
        }
        Ok((self.research(c).await?, false))
    }

    /// Research (after a clarity pre-screen, when research would be paid for), ask Jev, evaluate.
    ///
    /// Trades only come out of evidence researched in this call: a trade signal on a cached
    /// brief is researched again and re-evaluated. An edge above `suspicious_edge` is skipped.
    pub async fn decide(&self, c: &Candidate, fresh: bool) -> Result<Decided> {
        let cached = if fresh { None } else { self.cached_brief(c)? };
        let from_cache = cached.is_some();
        let brief = match cached {
            Some(b) => Some(b),
            None if self.researcher.as_ref().is_some_and(Researcher::budget_left) => {
                if let Some(reason) = self.prescreen(c).await? {
                    return Ok(Decided::Unclear(reason));
                }
                self.research(c).await?
            }
            None => None,
        };
        let mut a = self.assess(c, brief, from_cache).await?;
        let mut verdict = evaluate(&a.view, &c.book, self.s);

        if a.cached && matches!(verdict, Verdict::Trade(_)) {
            tracing::info!("{}: trade signal on a cached brief, researching again", c.market.slug);
            match self.research(c).await? {
                Some(b) => {
                    a = self.assess(c, Some(b), false).await?;
                    verdict = evaluate(&a.view, &c.book, self.s);
                }
                None => verdict = Verdict::Skip("cached brief could not be refreshed before trading".into()),
            }
        }
        if let Verdict::Trade(t) = &verdict
            && t.edge > self.s.suspicious_edge
        {
            tracing::warn!("{}: suspicious edge, {}", c.market.slug, t.rationale);
            verdict = Verdict::Skip(format!(
                "edge {:+.3} > suspicious_edge {}: more likely a model error than a mispricing",
                t.edge, self.s.suspicious_edge
            ));
        }
        Ok(Decided::Assessed(Box::new(a), verdict))
    }

    /// The cached brief younger than `research_ttl_hours`, unless the midpoint has since moved
    /// more than `research_max_price_move`: the market has likely seen news the brief lacks.
    fn cached_brief(&self, c: &Candidate) -> Result<Option<Brief>> {
        if self.researcher.is_none() {
            return Ok(None);
        }
        let slug = &c.market.slug;
        let Some((brief, then)) = self.store.get_brief(slug, self.s.research_ttl_hours * 3600.0)? else {
            return Ok(None);
        };
        if let (Some(then), Some(now)) = (then, c.book.midpoint())
            && (now - then).abs() > self.s.research_max_price_move
        {
            tracing::info!("{slug}: midpoint moved {then:.2} -> {now:.2} since the cached brief");
            return Ok(None);
        }
        Ok(Some(brief))
    }

    /// A new brief, cached with the current midpoint. See [`Pipeline::brief`] for when there is none.
    async fn research(&self, c: &Candidate) -> Result<Option<Brief>> {
        let Some(researcher) = &self.researcher else { return Ok(None) };
        let m = &c.market;
        if !researcher.budget_left() {
            tracing::info!("{}: research budget exhausted for this run", m.slug);
            return Ok(None);
        }
        let description: String = m.description.chars().take(self.s.description_max_chars * 2).collect();
        let topic = Topic {
            question: &m.question,
            description: &description,
            resolution_source: m.resolution_source.as_deref(),
            end_date: m.end_date.map(|d| d.date_naive().to_string()),
            today: today(),
        };
        match researcher.brief(&topic).await {
            Ok(b) => {
                self.store.put_brief(&m.slug, &b, c.book.midpoint())?;
                Ok(Some(b))
            }
            Err(e) if e.is_fatal() => Err(e.into()),
            Err(e) => {
                tracing::warn!("{}: research failed: {e}", m.slug);
                Ok(None)
            }
        }
    }

    /// Ask Jev for clarity alone, on the state without evidence. Returns (and logs) the skip
    /// reason when the market is too unclear to be worth a brief.
    async fn prescreen(&self, c: &Candidate) -> Result<Option<String>> {
        let state = build_state(c, self.s, None);
        let v = ask_clarity(&self.jev, &state).await?;
        if v.clarity >= self.s.min_clarity {
            return Ok(None);
        }
        let reason = format!("clarity {} < {} (pre-screen, not researched)", v.clarity, self.s.min_clarity);
        self.store.log_decision(&DecisionRow {
            slug: &c.market.slug,
            condition_id: &c.market.condition_id,
            question: &c.market.question,
            state: &state,
            p_yes: None,
            answerable: None,
            clarity: v.clarity,
            yes_ask: c.book.yes_ask,
            no_ask: c.book.no_ask,
            midpoint: c.book.midpoint(),
            edge: None,
            action: "skip",
            reason: &reason,
            jev_model: v.model.as_deref(),
            jev_cost: v.cost,
            research_cost: None,
            raw: &v.raw,
        })?;
        Ok(Some(reason))
    }

    async fn assess(&self, c: &Candidate, brief: Option<Brief>, cached: bool) -> Result<Assessment> {
        let state = build_state(c, self.s, brief.as_ref());
        let view = ask_jev(&self.jev, &state).await?;
        Ok(Assessment { state, brief, cached, view })
    }

    pub fn log(&self, c: &Candidate, a: &Assessment, verdict: &Verdict, executed: bool) -> Result<()> {
        let (action, edge, reason) = match verdict {
            Verdict::Trade(t) => {
                (if executed { "trade" } else { "trade_unexecuted" }, Some(t.edge), t.rationale.as_str())
            }
            Verdict::Skip(why) => ("skip", None, why.as_str()),
        };
        self.store.log_decision(&DecisionRow {
            slug: &c.market.slug,
            condition_id: &c.market.condition_id,
            question: &c.market.question,
            state: &a.state,
            p_yes: Some(a.view.p_yes),
            answerable: Some(a.view.answerable),
            clarity: a.view.clarity,
            yes_ask: c.book.yes_ask,
            no_ask: c.book.no_ask,
            midpoint: c.book.midpoint(),
            edge,
            action,
            reason,
            jev_model: a.view.model.as_deref(),
            jev_cost: a.view.cost,
            research_cost: a.brief.as_ref().map(|b| b.cost),
            raw: &a.view.raw,
        })
    }

    pub fn reset_budget(&self) {
        if let Some(r) = &self.researcher {
            r.reset_budget();
        }
    }

    /// One-line spend summary for this pass.
    pub fn spend(&self) -> String {
        let jev = &self.jev;
        let mut line = format!(
            "Jev: {} calls, {} tokens, ${:.5}",
            jev.calls.get(),
            jev.total_input_tokens.get(),
            jev.total_cost.get()
        );
        if let Some(r) = &self.researcher {
            line += &format!(" | researcher: {} briefs, ${:.4}", r.calls.get(), r.total_cost.get());
        }
        line
    }

    /// One trading pass: resolutions, scan, then decide `concurrency` markets ahead while
    /// orders go out one at a time in scan order, so the caps see every earlier fill. Resolving
    /// `stop` ends the pass after the current market: it drops research and Jev calls in flight,
    /// never an order. `on` hears every step, after it is logged.
    pub async fn pass(
        &self,
        ex: &mut Executor<'_>,
        gamma: &gamma::Client,
        clob: &clob::Client,
        limit: usize,
        stop: impl Future<Output = ()>,
        mut on: impl FnMut(Step<'_>),
    ) -> Result<()> {
        let (s, store) = (self.s, self.store);
        ex.trades_this_run = 0;
        self.reset_budget();
        match update_resolutions(gamma, store).await {
            Ok(0) => {}
            Ok(n) => on(Step::Resolved(n)),
            Err(e) => tracing::warn!("resolution check failed: {e:#}"),
        }
        let cands = scan(gamma, clob, s, limit, PAGES).await?;
        let exposure = ex.exposure(true).await?;
        let exposure_usd = exposure.total();
        let mut todo = Vec::with_capacity(cands.len());
        for c in cands {
            if exposure.condition_ids.contains(&c.market.condition_id)
                || store.has_order_for(&c.market.condition_id, false)?
            {
                on(Step::AlreadyExposed(&c));
            } else {
                todo.push(c);
            }
        }
        on(Step::Scanned { candidates: todo.len(), exposure_usd });

        let mut decided = futures::stream::iter(todo)
            .map(|c| async move {
                let d = self.decide(&c, false).await;
                (c, d)
            })
            .buffered(s.concurrency.max(1));
        let mut stop = std::pin::pin!(stop.fuse());
        loop {
            let next = futures::select_biased! {
                () = stop => {
                    on(Step::Stopped);
                    break;
                }
                next = decided.next() => next,
            };
            let Some((c, d)) = next else { break };
            match d {
                Err(e) if is_fatal(&e) => return Err(e),
                Err(e) => on(Step::Failed(&c, &e)),
                Ok(Decided::Unclear(reason)) => on(Step::Unclear(&c, &reason)),
                Ok(Decided::Assessed(a, verdict)) => {
                    let placed = match &verdict {
                        Verdict::Trade(t) => Some(ex.place(&c, t, false).await?),
                        Verdict::Skip(_) => None,
                    };
                    self.log(&c, &a, &verdict, placed.as_ref().is_some_and(|p| p.ok))?;
                    on(Step::Decided { c: &c, a: &a, verdict: &verdict, placed: placed.as_ref() });
                }
            }
            if ex.trades_this_run >= s.max_trades_per_run {
                on(Step::TradeCapReached);
                break;
            }
        }
        Ok(())
    }
}

/// Market pages (50 each) a pass scans.
const PAGES: u32 = 5;

/// What [`Pipeline::pass`] reports as it goes.
pub enum Step<'a> {
    Resolved(usize),
    AlreadyExposed(&'a Candidate),
    Scanned {
        candidates: usize,
        exposure_usd: f64,
    },
    Unclear(&'a Candidate, &'a str),
    Failed(&'a Candidate, &'a anyhow::Error),
    /// Logged; `placed` is set when the verdict was a trade.
    Decided {
        c: &'a Candidate,
        a: &'a Assessment,
        verdict: &'a Verdict,
        placed: Option<&'a Placed>,
    },
    TradeCapReached,
    Stopped,
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::markets::test_candidate;

    /// Jev answers `p_yes` with the given clarity; the researcher is expected `briefs` times.
    async fn server(p_yes: f64, clarity: u8, briefs: u64) -> MockServer {
        let server = MockServer::start().await;
        let answers =
            json!({"resolves_yes": {"noul": p_yes}, "answerable": {"noul": 0.9}, "clarity": {"score": clarity}});
        Mock::given(path("/alpha/decisions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"answers": answers, "usage": {"cost": 0.00003}})),
            )
            .mount(&server)
            .await;
        let brief = json!({"as_of": "2026-09-20", "summary": "fresh", "key_facts": ["2026-09-20: news"]});
        let completion = json!({"choices": [{"message": {"content": brief.to_string()}}], "usage": {"cost": 0.01}});
        Mock::given(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(completion))
            .expect(briefs)
            .mount(&server)
            .await;
        server
    }

    fn setup(server: &MockServer) -> (tempfile::TempDir, Store, Settings) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("t.db")).unwrap();
        let s = Settings { openrouter_api_key: "k".into(), openrouter_base_url: server.uri(), ..Settings::default() };
        (dir, store, s)
    }

    fn cache(store: &Store, midpoint: f64) {
        let brief = Brief { summary: "cached".into(), ..Brief::default() };
        store.put_brief("m", &brief, Some(midpoint)).unwrap();
    }

    fn assessed(d: Decided) -> (Assessment, Verdict) {
        match d {
            Decided::Assessed(a, v) => (*a, v),
            Decided::Unclear(why) => panic!("expected an assessment, got unclear: {why}"),
        }
    }

    #[tokio::test]
    async fn trade_on_cached_brief_is_researched_again() {
        let server = server(0.7, 3, 1).await;
        let (_dir, store, s) = setup(&server);
        let c = test_candidate(0.5, 10, 20_000.0);
        cache(&store, c.book.midpoint().unwrap());
        let p = Pipeline::new(&s, &store, Research::Unlimited).unwrap();

        let (a, verdict) = assessed(p.decide(&c, false).await.unwrap());
        assert!(matches!(verdict, Verdict::Trade(_)), "{verdict:?}");
        assert_eq!((a.cached, a.brief.unwrap().summary.as_str()), (false, "fresh"));
        assert_eq!(p.jev.calls.get(), 2, "once on the cached brief, once on the fresh one");
    }

    #[tokio::test]
    async fn unclear_market_is_skipped_before_research() {
        let server = server(0.7, 0, 0).await;
        let (_dir, store, s) = setup(&server);
        let p = Pipeline::new(&s, &store, Research::Unlimited).unwrap();

        let d = p.decide(&test_candidate(0.5, 10, 20_000.0), false).await.unwrap();
        assert!(matches!(d, Decided::Unclear(ref why) if why.contains("pre-screen")));
        assert_eq!(p.jev.calls.get(), 1);
        assert_eq!(store.stats().unwrap().decisions, 1, "the skip is logged");
    }

    #[tokio::test]
    async fn suspicious_edge_is_skipped_on_fresh_evidence() {
        let server = server(0.9, 3, 1).await;
        let (_dir, store, s) = setup(&server);
        let p = Pipeline::new(&s, &store, Research::Unlimited).unwrap();

        let (_, verdict) = assessed(p.decide(&test_candidate(0.5, 10, 20_000.0), false).await.unwrap());
        assert!(matches!(verdict, Verdict::Skip(ref why) if why.contains("suspicious_edge")), "{verdict:?}");
    }

    #[tokio::test]
    async fn cached_brief_is_dropped_after_a_price_move() {
        let server = server(0.5, 3, 1).await;
        let (_dir, store, s) = setup(&server);
        cache(&store, 0.30);
        let p = Pipeline::new(&s, &store, Research::Unlimited).unwrap();

        let (a, _) = assessed(p.decide(&test_candidate(0.5, 10, 20_000.0), false).await.unwrap());
        assert!(!a.cached, "midpoint 0.30 -> 0.49 exceeds research_max_price_move");
    }
}
