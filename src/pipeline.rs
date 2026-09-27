//! The one path from candidate to Jev view: research (cached) -> state -> Jev, plus logging.

use anyhow::Result;
use serde_json::Value;

use crate::config::Settings;
use crate::jev::JevClient;
use crate::markets::{Candidate, build_state, today};
use crate::openrouter::{OpenRouter, OpenRouterError};
use crate::research::{Brief, Researcher, Topic};
use crate::signal::{JevView, Verdict, ask_jev};
use crate::store::{DecisionRow, Store};

pub struct Assessment {
    pub state: Value,
    pub brief: Option<Brief>,
    /// The brief came from the cache (already paid for).
    pub cached: bool,
    pub view: JevView,
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

    /// `(brief, from_cache)`. No brief if research is disabled, over budget, or failed;
    /// only fatal OpenRouter errors (bad key, no credits) are returned as errors.
    pub async fn brief(&self, c: &Candidate, fresh: bool) -> Result<(Option<Brief>, bool)> {
        let Some(researcher) = &self.researcher else { return Ok((None, false)) };
        let m = &c.market;
        if !fresh && let Some(b) = self.store.get_brief(&m.slug, self.s.research_ttl_hours * 3600.0)? {
            return Ok((Some(b), true));
        }
        if !researcher.budget_left() {
            tracing::info!("{}: research budget exhausted for this run", m.slug);
            return Ok((None, false));
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
                self.store.put_brief(&m.slug, &b)?;
                Ok((Some(b), false))
            }
            Err(e) if e.is_fatal() => Err(e.into()),
            Err(e) => {
                tracing::warn!("{}: research failed: {e}", m.slug);
                Ok((None, false))
            }
        }
    }

    pub async fn assess(&self, c: &Candidate, fresh: bool) -> Result<Assessment> {
        let (brief, cached) = self.brief(c, fresh).await?;
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
            p_yes: a.view.p_yes,
            answerable: a.view.answerable,
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
}
