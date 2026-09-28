//! The trading loop. `Pipeline` and `Executor` borrow the settings and the store and count spend
//! in `Cell`s, so their futures are not `Send`. The engine therefore runs on its own thread with
//! a single-threaded runtime, owns everything that touches them, and takes commands over a
//! channel. Passes, single decisions and position lookups are futures polled side by side on
//! that one thread; HTTP handlers only ever see `Send` messages and replies.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use futures::future::{Fuse, FusedFuture as _, FutureExt as _};
use futures::stream::{FuturesUnordered, StreamExt as _};
use polymarket_client_sdk_v2::{clob, gamma};
use rust_decimal::prelude::ToPrimitive as _;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

use super::api::{
    BriefRecord, Event, Exposure, LoopState, OpenOrder, OrderEvent, Pass, Position, Positions, Recommendation, Spend,
};
use super::db::Db;
use super::hub::Hub;
use super::{Options, now};
use crate::config::{Paths, Settings};
use crate::executor::Executor;
use crate::markets::{Candidate, load_candidate};
use crate::pipeline::{Decided, Pipeline, Research, Step, is_fatal};
use crate::signal::Verdict;

/// Log target of pass start and end lines: the terminal gets them, the event bus skips them
/// because `PassStarted` and `PassFinished` carry the same news.
pub const PASS_LOG: &str = "jevmarket::daemon::pass";

type Reply<T> = oneshot::Sender<Result<T>>;
type Job<'a, T = ()> = Pin<Box<dyn Future<Output = T> + 'a>>;

pub enum Command {
    RunPass(Reply<()>),
    Pause,
    Resume,
    Decide { reference: String, fresh: bool, reply: Reply<Recommendation> },
    RefreshBrief { reference: String, reply: Reply<BriefRecord> },
    Positions(Reply<Positions>),
}

pub struct Engine {
    pub paths: Paths,
    pub opts: Options,
    pub hub: Arc<Hub>,
    pub db: Db,
    /// Set to end the running pass after its current market.
    stop: watch::Sender<bool>,
}

impl Engine {
    pub fn new(paths: Paths, opts: Options, hub: Arc<Hub>, db: Db) -> Self {
        Self { paths, opts, hub, db, stop: watch::channel(false).0 }
    }

    fn settings(&self) -> Result<Settings> {
        super::settings(&self.paths, self.opts.dry_run_forced)
    }

    fn ttl_s(s: &Settings) -> f64 {
        s.research_ttl_hours * 3600.0
    }

    /// Runs passes every `loop_secs` and serves commands until `shutdown` turns true. Starts
    /// paused when live. A pass in flight at shutdown finishes its current market; an order is
    /// never abandoned half-placed.
    pub async fn run(self, mut commands: mpsc::Receiver<Command>, mut shutdown: watch::Receiver<bool>) {
        let this = &self;
        let mut jobs: FuturesUnordered<Job<'_>> = FuturesUnordered::new();
        let mut pass: Fuse<Job<'_, bool>> = Fuse::terminated();
        let mut next = Instant::now();
        // Live money waits for a confirmed resume from the console.
        let mut paused = this.settings().is_ok_and(|s| !s.dry_run);
        if paused {
            tracing::warn!("LIVE: the loop starts paused; resume it in the console to place real orders");
        }
        let mut number = 0;
        let interval = Duration::from_secs(self.opts.loop_secs);
        loop {
            this.publish_schedule(paused, !pass.is_terminated(), next);
            tokio::select! {
                _ = shutdown.wait_for(|stop| *stop) => break,
                Some(command) = commands.recv() => match command {
                    Command::RunPass(reply) => {
                        let started = if pass.is_terminated() {
                            number += 1;
                            pass = this.start(number);
                            Ok(())
                        } else {
                            Err(anyhow::anyhow!("a pass is already running"))
                        };
                        let _ = reply.send(started);
                    }
                    Command::Pause => {
                        paused = true;
                        if !pass.is_terminated() {
                            this.stop.send_replace(true);
                        }
                        tracing::info!("loop paused");
                    }
                    Command::Resume => {
                        paused = false;
                        tracing::info!("loop resumed");
                    }
                    Command::Decide { reference, fresh, reply } => {
                        jobs.push(Box::pin(async move { let _ = reply.send(this.decide(&reference, fresh).await); }));
                    }
                    Command::RefreshBrief { reference, reply } => {
                        jobs.push(Box::pin(async move { let _ = reply.send(this.refresh_brief(&reference).await); }));
                    }
                    Command::Positions(reply) => {
                        jobs.push(Box::pin(async move { let _ = reply.send(this.positions().await); }));
                    }
                },
                Some(()) = jobs.next(), if !jobs.is_empty() => {}
                fatal = &mut pass, if !pass.is_terminated() => {
                    paused |= fatal;
                    next = Instant::now() + interval;
                }
                () = tokio::time::sleep_until(next), if pass.is_terminated() && !paused => {
                    number += 1;
                    pass = this.start(number);
                }
            }
        }
        if !pass.is_terminated() {
            tracing::info!("finishing the current market before shutdown");
            this.stop.send_replace(true);
            this.publish_schedule(true, true, next);
            (&mut pass).await;
        }
    }

    fn publish_schedule(&self, paused: bool, running: bool, next: Instant) {
        let stopping = running && *self.stop.borrow();
        let state = match (running, stopping, paused) {
            (true, true, _) => LoopState::Stopping,
            (true, false, _) => LoopState::Running,
            (false, _, true) => LoopState::Paused,
            (false, _, false) => LoopState::Waiting,
        };
        let next_pass_at =
            (state == LoopState::Waiting).then(|| now() + next.saturating_duration_since(Instant::now()).as_secs_f64());
        self.hub.update(|s| {
            s.state = state;
            // Keep the timestamp stable between updates, so an unchanged schedule is not re-sent.
            if next_pass_at.is_none_or(|t| s.next_pass_at.is_none_or(|old| (old - t).abs() > 1.0)) {
                s.next_pass_at = next_pass_at;
            }
        });
    }

    fn start(&self, number: u64) -> Fuse<Job<'_, bool>> {
        self.stop.send_replace(false);
        self.pass(number).boxed_local().fuse()
    }

    /// One pass, start to finish, with its status and events. Returns whether the error (if
    /// any) means every further pass would fail too, so the loop should pause.
    async fn pass(&self, number: u64) -> bool {
        let mut p = Pass { number, started_at: now(), ..Pass::default() };
        let result = self.run_pass(&mut p).await;
        p.finished_at = Some(now());
        let fatal = result.as_ref().is_err_and(is_fatal);
        p.error = result.err().map(|e| format!("{e:#}"));
        match &p.error {
            Some(e) if fatal => tracing::error!(target: PASS_LOG, pass = number, "pass failed, loop paused: {e}"),
            Some(e) => tracing::warn!(target: PASS_LOG, pass = number, "pass failed: {e}"),
            None => {
                tracing::info!(target: PASS_LOG, pass = number, assessed = p.assessed, trades = p.trades, "pass finished")
            }
        }
        self.hub.update(|s| {
            s.pass = None;
            s.last_pass = Some(p.clone());
            s.last_error.clone_from(&p.error);
        });
        self.hub.emit(Event::PassFinished { pass: p });
        self.hub.emit(Event::StatsChanged);
        self.hub.emit(Event::PositionsChanged);
        fatal
    }

    async fn run_pass(&self, p: &mut Pass) -> Result<()> {
        let s = self.settings()?;
        p.dry_run = s.dry_run;
        self.hub.update(|st| {
            st.pass = Some(p.clone());
            st.dry_run = s.dry_run;
        });
        self.hub.emit(Event::PassStarted { pass: p.clone() });
        tracing::info!(target: PASS_LOG, pass = p.number, dry_run = s.dry_run, "pass started");

        let research = if s.research_enabled { Research::Budget(s.max_research_per_run) } else { Research::Off };
        let pipeline = Pipeline::new(&s, &self.db.store, research)?;
        let result = self.trade(&s, &pipeline, p).await;
        p.spend = spend(&pipeline);
        self.hub.update(|st| st.spend_total += p.spend);
        result
    }

    /// The shared pass from [`Pipeline::pass`], with its steps on the event stream.
    async fn trade(&self, s: &Settings, pipeline: &Pipeline<'_>, p: &mut Pass) -> Result<()> {
        let mut ex = Executor::create(s, &self.db.store, s.dry_run).await?;
        let (gamma, clob) = clients(s)?;
        let mut stop = self.stop.subscribe();
        let stop = async move {
            let _ = stop.wait_for(|stop| *stop).await;
        };
        pipeline
            .pass(&mut ex, &gamma, &clob, self.opts.limit, stop, |step| {
                match step {
                    Step::Resolved(n) => tracing::info!(resolutions = n, "markets resolved since the last pass"),
                    Step::AlreadyExposed(c) => tracing::info!(slug = %c.market.slug, "already exposed, skip"),
                    Step::Scanned { candidates, .. } => p.candidates = candidates,
                    Step::Unclear(c, _) => {
                        p.assessed += 1;
                        self.publish_decision(&c.market.slug);
                    }
                    Step::Failed(c, e) => {
                        p.assessed += 1;
                        tracing::warn!(slug = %c.market.slug, "{e:#}");
                    }
                    Step::Decided { c, a, verdict, placed } => {
                        p.assessed += 1;
                        if let (Verdict::Trade(t), Some(placed)) = (verdict, placed) {
                            p.trades += u32::from(placed.ok);
                            self.hub.emit(Event::Order {
                                order: OrderEvent {
                                    ts: now(),
                                    slug: c.market.slug.clone(),
                                    outcome: t.outcome.to_string(),
                                    price: t.price,
                                    size: t.size,
                                    usd: t.usd,
                                    status: placed.status.clone(),
                                    dry_run: s.dry_run,
                                    order_id: placed.order_id.clone(),
                                    message: placed.message.clone(),
                                },
                            });
                        }
                        if a.brief.is_some() && !a.cached {
                            self.publish_brief(&c.market.slug, s);
                        }
                        self.publish_decision(&c.market.slug);
                    }
                    Step::TradeCapReached => tracing::info!(pass = p.number, "trade cap for this pass reached"),
                    Step::Stopped => tracing::info!(pass = p.number, "pass stopped early"),
                }
                p.spend = spend(pipeline);
                self.hub.update(|st| st.pass = Some(p.clone()));
            })
            .await
    }

    /// Research and ask Jev about one market, log it, never trade. Like `jevmarket decide`.
    async fn decide(&self, reference: &str, fresh: bool) -> Result<Recommendation> {
        let s = self.settings()?;
        let research = if s.research_enabled { Research::Unlimited } else { Research::Off };
        let pipeline = Pipeline::new(&s, &self.db.store, research)?;
        let c = self.candidate(&s, reference).await?;
        let decided = pipeline.decide(&c, fresh).await;
        self.hub.update(|st| st.spend_total += spend(&pipeline));
        if let Decided::Assessed(a, verdict) = decided? {
            pipeline.log(&c, &a, &verdict, false)?;
            if a.brief.is_some() && !a.cached {
                self.publish_brief(&c.market.slug, &s);
            }
        }
        let r = self.db.recommendation(&c.market.slug)?.context("the decision was not logged")?;
        self.hub.emit(Event::Decision { recommendation: r.clone() });
        Ok(r)
    }

    /// A new brief for one market, researched even when `research_enabled` is off: it was asked for.
    async fn refresh_brief(&self, reference: &str) -> Result<BriefRecord> {
        let s = self.settings()?;
        let pipeline = Pipeline::new(&s, &self.db.store, Research::Unlimited)?;
        let c = self.candidate(&s, reference).await?;
        let researched = pipeline.brief(&c, true).await;
        self.hub.update(|st| st.spend_total += spend(&pipeline));
        researched?.0.context("the researcher returned no brief, see the log")?;
        self.publish_brief(&c.market.slug, &s);
        let latest = self.db.recommendation_detail(&c.market.slug, Self::ttl_s(&s))?.and_then(|d| d.brief);
        latest.context("the brief was not stored")
    }

    async fn candidate(&self, s: &Settings, reference: &str) -> Result<Candidate> {
        let (gamma, clob) = clients(s)?;
        load_candidate(&gamma, &clob, reference).await
    }

    async fn positions(&self) -> Result<Positions> {
        let s = self.settings()?;
        // Read-only: a dry-run executor never places orders.
        let mut ex = Executor::create(&s, &self.db.store, true).await?;
        let d = |x: rust_decimal::Decimal| x.to_f64().unwrap_or_default();
        let positions = ex
            .positions()
            .await?
            .into_iter()
            .map(|p| Position {
                slug: p.slug,
                title: p.title,
                outcome: p.outcome,
                size: d(p.size),
                avg_price: d(p.avg_price),
                cur_price: d(p.cur_price),
                value_usd: d(p.current_value),
                pnl_usd: d(p.cash_pnl),
                redeemable: p.redeemable,
            })
            .collect();
        let open_orders = ex
            .open_orders()
            .await?
            .into_iter()
            .map(|o| OpenOrder {
                id: o.id,
                market: o.market.to_string(),
                side: o.side.to_string(),
                outcome: o.outcome,
                price: d(o.price),
                size: d(o.original_size),
                matched: d(o.size_matched),
                status: o.status.to_string(),
                created_at: o.created_at.timestamp() as f64,
            })
            .collect();
        let balance_usd = ex.collateral_balance_usd().await;
        let e = ex.exposure(true).await?;
        let exposure = Exposure {
            positions_usd: e.positions_usd,
            open_orders_usd: e.open_orders_usd,
            total_usd: e.total(),
            cap_usd: s.max_open_exposure_usd,
        };
        Ok(Positions {
            wallet: ex.wallet.map(|w| w.to_string()),
            wallet_type: ex.wallet_type(),
            balance_usd,
            positions,
            open_orders,
            exposure,
        })
    }

    fn publish_decision(&self, slug: &str) {
        match self.db.recommendation(slug) {
            Ok(Some(recommendation)) => self.hub.emit(Event::Decision { recommendation }),
            Ok(None) => {}
            Err(e) => tracing::warn!(slug, "reading the decision back failed: {e:#}"),
        }
    }

    fn publish_brief(&self, slug: &str, s: &Settings) {
        match self.db.latest_brief(slug, Self::ttl_s(s)) {
            Ok(Some(brief)) => self.hub.emit(Event::Brief { brief }),
            Ok(None) => {}
            Err(e) => tracing::warn!(slug, "reading the brief back failed: {e:#}"),
        }
    }
}

fn clients(s: &Settings) -> Result<(gamma::Client, clob::Client)> {
    Ok((gamma::Client::default(), clob::Client::new(&s.clob_host, clob::Config::default())?))
}

fn spend(p: &Pipeline<'_>) -> Spend {
    let r = p.researcher.as_ref();
    Spend {
        jev_calls: p.jev.calls.get(),
        jev_usd: p.jev.total_cost.get(),
        briefs: r.map_or(0, |r| r.calls.get()),
        research_usd: r.map_or(0.0, |r| r.total_cost.get()),
    }
}
