//! The trading loop. `Pipeline` and `Executor` borrow the settings and the store and count spend
//! in `Cell`s, so their futures are not `Send`. The engine therefore runs on its own thread with
//! a single-threaded runtime, owns everything that touches them, and takes commands over a
//! channel. Passes, price-watch ticks, single decisions and position lookups are futures polled
//! side by side on that one thread; HTTP handlers only ever see `Send` messages and replies.
//!
//! Two cadences share the loop: research cycles (passes) every `--loop` seconds, and price-watch
//! ticks every `watch_interval_secs` in between. They never run at the same time: a tick waits
//! out a cycle, and a cycle that falls due during a tick starts right after it. Pausing stops the
//! cycles and every order; the watch keeps checking prices.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
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
use super::price_watch::{self, Acted, Held, Tick, Window};
use super::{Options, now};
use crate::config::{Paths, Settings};
use crate::executor::{self, Executor, Placed};
use crate::markets::{Candidate, load_candidate, load_candidates};
use crate::pipeline::{self, Decided, Pipeline, Research, Step, is_fatal};
use crate::signal::{Exit, Holding, Outcome, Trade, Verdict, manual_trade};

/// Log target of pass start and end lines: the terminal gets them, the event bus skips them
/// because `PassStarted` and `PassFinished` carry the same news.
pub const PASS_LOG: &str = "jevmarket::daemon::pass";

type Reply<T> = oneshot::Sender<Result<T>>;
type Job<'a, T = ()> = Pin<Box<dyn Future<Output = T> + 'a>>;

pub enum Command {
    RunPass(Reply<()>),
    Pause,
    Resume,
    Decide {
        reference: String,
        fresh: bool,
        reply: Reply<Recommendation>,
    },
    RefreshBrief {
        reference: String,
        reply: Reply<BriefRecord>,
    },
    Positions(Reply<Positions>),
    Order {
        order: ManualOrder,
        reply: Reply<OrderEvent>,
    },
    /// Put a market on the watchlist, deciding it first when Jev never priced it.
    Watch {
        reference: String,
        reply: Reply<Recommendation>,
    },
}

/// A BUY a person asked for in the console.
#[derive(Debug, serde::Deserialize)]
pub struct ManualOrder {
    /// Slug or polymarket.com URL.
    pub reference: String,
    pub outcome: Outcome,
    /// Limit price.
    pub price: f64,
    /// Roughly what to spend; the size is rounded like the signal's trades.
    pub usd: f64,
}

pub struct Engine {
    pub paths: Paths,
    pub opts: Options,
    pub hub: Arc<Hub>,
    pub db: Db,
    /// Set to end the running pass or watch tick after its current market.
    stop: watch::Sender<bool>,
    /// Orders, briefs and exposure since the research cycle started, shared with the watch.
    window: RefCell<Window>,
}

/// How often the settings are read again while the price watch is off.
const WATCH_RECHECK: Duration = Duration::from_secs(60);

impl Engine {
    pub fn new(paths: Paths, opts: Options, hub: Arc<Hub>, db: Db) -> Self {
        Self { paths, opts, hub, db, stop: watch::channel(false).0, window: RefCell::default() }
    }

    fn settings(&self) -> Result<Settings> {
        super::settings(&self.paths, self.opts.dry_run_forced)
    }

    fn ttl_s(s: &Settings) -> f64 {
        s.research_ttl_hours * 3600.0
    }

    /// Runs research cycles every `loop_secs` and watch ticks in between, and serves commands
    /// until `shutdown` turns true. Starts paused when live. A pass or tick in flight at shutdown
    /// finishes its current market; an order is never abandoned half-placed.
    pub async fn run(self, mut commands: mpsc::Receiver<Command>, mut shutdown: watch::Receiver<bool>) {
        let this = &self;
        let mut jobs: FuturesUnordered<Job<'_>> = FuturesUnordered::new();
        let mut pass: Fuse<Job<'_, bool>> = Fuse::terminated();
        let mut tick: Fuse<Job<'_>> = Fuse::terminated();
        let mut next = Instant::now();
        let (mut next_tick, mut watching) = this.schedule_watch();
        // A pass asked for while a tick ran, started once it is done.
        let mut pass_asked = false;
        // Live money waits for a confirmed resume from the console, unless `--autostart` said so.
        let live = this.settings().is_ok_and(|s| !s.dry_run);
        let mut paused = live && !self.opts.autostart;
        if paused {
            tracing::warn!("LIVE: the loop starts paused; resume it in the console to place real orders");
        } else if live {
            tracing::warn!("LIVE: --autostart, the loop places real orders from the start");
        }
        let mut number = 0;
        let interval = Duration::from_secs(self.opts.loop_secs);
        loop {
            let idle = pass.is_terminated() && tick.is_terminated();
            this.publish_schedule(paused, !pass.is_terminated(), next, (watching && idle).then_some(next_tick));
            tokio::select! {
                _ = shutdown.wait_for(|stop| *stop) => break,
                Some(command) = commands.recv() => match command {
                    Command::RunPass(reply) => {
                        let started = if !pass.is_terminated() {
                            Err(anyhow::anyhow!("a pass is already running"))
                        } else if !tick.is_terminated() {
                            pass_asked = true;
                            Ok(())
                        } else {
                            number += 1;
                            pass = this.start(number);
                            Ok(())
                        };
                        let _ = reply.send(started);
                    }
                    Command::Pause => {
                        paused = true;
                        if !idle {
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
                    Command::Order { order, reply } => {
                        jobs.push(Box::pin(async move { let _ = reply.send(this.order(order).await); }));
                    }
                    Command::Watch { reference, reply } => {
                        // The next tick picks the market up right away.
                        if watching {
                            next_tick = Instant::now();
                        }
                        jobs.push(Box::pin(async move { let _ = reply.send(this.watch_market(&reference).await); }));
                    }
                },
                Some(()) = jobs.next(), if !jobs.is_empty() => {}
                fatal = &mut pass, if !pass.is_terminated() => {
                    paused |= fatal;
                    next = Instant::now() + interval;
                    (next_tick, watching) = this.schedule_watch();
                }
                () = &mut tick, if !tick.is_terminated() => {
                    (next_tick, watching) = this.schedule_watch();
                    if std::mem::take(&mut pass_asked) {
                        number += 1;
                        pass = this.start(number);
                    }
                }
                () = tokio::time::sleep_until(next), if idle && !paused => {
                    number += 1;
                    pass = this.start(number);
                }
                () = tokio::time::sleep_until(next_tick), if idle => {
                    if watching {
                        this.stop.send_replace(false);
                        tick = this.watch(!paused).boxed_local().fuse();
                    } else {
                        (next_tick, watching) = this.schedule_watch();
                    }
                }
            }
        }
        if !(pass.is_terminated() && tick.is_terminated()) {
            tracing::info!("finishing the current market before shutdown");
            this.stop.send_replace(true);
            this.publish_schedule(true, !pass.is_terminated(), next, None);
            if !pass.is_terminated() {
                (&mut pass).await;
            }
            if !tick.is_terminated() {
                (&mut tick).await;
            }
        }
    }

    fn publish_schedule(&self, paused: bool, running: bool, next: Instant, next_tick: Option<Instant>) {
        let stopping = running && *self.stop.borrow();
        let state = match (running, stopping, paused) {
            (true, true, _) => LoopState::Stopping,
            (true, false, _) => LoopState::Running,
            (false, _, true) => LoopState::Paused,
            (false, _, false) => LoopState::Waiting,
        };
        let at = |i: Instant| now() + i.saturating_duration_since(Instant::now()).as_secs_f64();
        let next_pass_at = (state == LoopState::Waiting).then(|| at(next));
        self.hub.update(|s| {
            s.state = state;
            steady(&mut s.next_pass_at, next_pass_at);
            steady(&mut s.watch.next_tick_at, next_tick.map(at));
        });
    }

    /// When the next watch tick is due, and whether the watch is on. While it is off, the
    /// settings are read again after [`WATCH_RECHECK`] and nothing else happens.
    fn schedule_watch(&self) -> (Instant, bool) {
        let secs = self.settings().map_or(0, |s| s.watch_interval_secs);
        self.hub.update(|st| st.watch.interval_secs = secs);
        if secs == 0 {
            self.hub.set_watchlist(Vec::new());
            return (Instant::now() + WATCH_RECHECK, false);
        }
        (Instant::now() + Duration::from_secs(secs), true)
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
        // A cycle starts a new window for the trade and research caps it shares with the watch.
        self.window.take();
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
        let result = pipeline
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
                            self.hub.emit(Event::Order { order: order_event(c, t, placed, s.dry_run, false) });
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
            .await;
        let mut window = self.window.borrow_mut();
        window.trades = ex.trades_this_run;
        window.briefs = spend(pipeline).briefs;
        result
    }

    /// One price-watch tick with its status and events. Without `acting`, it only looks.
    async fn watch(&self, acting: bool) {
        let started = now();
        let result = self.watch_tick(acting).await;
        if let Err(e) = &result {
            tracing::warn!("watch tick failed: {e:#}");
        }
        self.hub.update(|st| {
            let w = &mut st.watch;
            w.last_tick_at = Some(started);
            match &result {
                Ok(t) => {
                    (w.watched, w.signals, w.error) = (t.items.len(), t.signals.len(), None);
                    if let Some(last) = t.signals.last() {
                        w.last_signal = Some(last.clone());
                    }
                }
                Err(e) => w.error = Some(format!("{e:#}")),
            }
        });
        if let Ok(t) = result {
            self.hub.set_watchlist(t.items);
            self.hub.update(|st| st.spend_total += t.spend);
            if t.decisions > 0 {
                self.hub.emit(Event::StatsChanged);
            }
            if t.orders > 0 {
                self.hub.emit(Event::PositionsChanged);
            }
        }
        self.hub.emit(Event::WatchlistChanged);
    }

    /// The stored views and held positions against live books, see [`price_watch::tick`]. Views on
    /// markets the wallet is known to be exposed to are left out before any book is fetched.
    async fn watch_tick(&self, acting: bool) -> Result<Tick> {
        let s = self.settings()?;
        let mut window = self.window.take();
        let result = async {
            let held = if s.sell_early { self.held(&s).await } else { Vec::new() };
            window.exposed.extend(held.iter().map(|h| h.view.condition_id.clone()));
            let mut views = self.db.watch_views(&s)?;
            views.retain(|v| !window.exposed.contains(&v.condition_id));
            let ids: Vec<String> =
                views.iter().chain(held.iter().map(|h| &h.view)).map(|v| v.condition_id.clone()).collect();
            let cands = if ids.is_empty() {
                Vec::new()
            } else {
                let (gamma, clob) = clients(&s)?;
                load_candidates(&gamma, &clob, &ids).await?
            };
            let stop = || *self.stop.borrow();
            let on = |c: &Candidate, acted: &Acted| self.publish_acted(&s, c, acted);
            price_watch::tick(&views, &held, &cands, &s, &self.db.store, &mut window, acting, stop, on).await
        }
        .await;
        self.window.replace(window);
        result
    }

    /// The wallet's open positions that Jev priced and the bot has not sold yet. Read from the
    /// public positions API without signing in; a failed read only skips exits for this tick.
    async fn held(&self, s: &Settings) -> Vec<Held> {
        let Some(user) = executor::wallet(s) else { return Vec::new() };
        let positions = match executor::positions(&Default::default(), user).await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("watch: {e:#}");
                return Vec::new();
            }
        };
        let d = |x: rust_decimal::Decimal| x.to_f64().unwrap_or_default();
        let open: Vec<_> = positions.into_iter().filter(|p| !p.redeemable && d(p.size) > 0.0).collect();
        let ids: Vec<String> = open.iter().map(|p| p.condition_id.to_string()).collect();
        let mut views: std::collections::HashMap<String, _> = match self.db.exit_views(&ids) {
            Ok(v) => v.into_iter().map(|v| (v.condition_id.clone(), v)).collect(),
            Err(e) => {
                tracing::warn!("watch: reading the views of held markets: {e:#}");
                return Vec::new();
            }
        };
        open.into_iter()
            .filter_map(|p| {
                let outcome = match p.outcome_index {
                    0 => Outcome::Yes,
                    1 => Outcome::No,
                    _ => return None,
                };
                let view = views.remove(&p.condition_id.to_string())?;
                if self.db.sold(&view.condition_id, s.dry_run).unwrap_or(true) {
                    return None;
                }
                let holding = Holding { outcome, token_id: p.asset, size: d(p.size), avg_price: d(p.avg_price) };
                Some(Held { view, holding })
            })
            .collect()
    }

    fn publish_acted(&self, s: &Settings, c: &Candidate, acted: &Acted) {
        let slug = c.market.slug.as_str();
        match acted {
            Acted::Passed(why) => tracing::debug!(slug, "watch signal left alone: {why}"),
            Acted::Exited { a, exit, placed } => {
                match (exit, placed) {
                    (Ok(e), Some(placed)) => self.hub.emit(Event::Order { order: exit_event(c, e, placed, s.dry_run) }),
                    (Err(why), _) => tracing::info!(slug, "holding on after a fresh look: {why}"),
                    (Ok(_), None) => {}
                }
                if a.brief.is_some() && !a.cached {
                    self.publish_brief(slug, s);
                }
                self.publish_decision(slug);
            }
            Acted::Decided { a, verdict, placed } => {
                if let (Verdict::Trade(t), Some(placed)) = (verdict, placed) {
                    self.hub.emit(Event::Order { order: order_event(c, t, placed, s.dry_run, false) });
                }
                if a.brief.is_some() && !a.cached {
                    self.publish_brief(slug, s);
                }
                self.publish_decision(slug);
            }
        }
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
            pipeline::log(&self.db.store, &c, &a, &verdict, false)?;
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

    /// Place a manual order on the current book, logged with `source = 'manual'`. Refused and
    /// rejected orders are errors, after they reached the activity feed.
    async fn order(&self, o: ManualOrder) -> Result<OrderEvent> {
        let s = self.settings()?;
        let c = self.candidate(&s, &o.reference).await?;
        let t = manual_trade(&c.book, o.outcome, o.price, o.usd).map_err(anyhow::Error::msg)?;
        let mut ex = Executor::create(&s, &self.db.store, s.dry_run).await?;
        let placed = ex.place(&c, &t, true).await?;
        let order = order_event(&c, &t, &placed, s.dry_run, true);
        self.hub.emit(Event::Order { order: order.clone() });
        if !placed.ok {
            bail!("order {}: {}", placed.status, placed.message.unwrap_or_default());
        }
        self.hub.emit(Event::PositionsChanged);
        self.hub.emit(Event::StatsChanged);
        Ok(order)
    }

    /// Put a market on the watchlist. One Jev never priced is decided first, like `decide`; one
    /// whose rules were too unclear to price is refused.
    async fn watch_market(&self, reference: &str) -> Result<Recommendation> {
        let s = self.settings()?;
        let c = self.candidate(&s, reference).await?;
        let slug = &c.market.slug;
        if self.db.recommendation(slug)?.is_none_or(|r| r.p_yes.is_none()) {
            self.decide(slug, false).await?;
        }
        let r = self.db.recommendation(slug)?.context("the decision was not logged")?;
        if r.p_yes.is_none() {
            bail!("Jev found the rules too unclear to price, so there is nothing to watch: {}", r.reason);
        }
        self.db.store.watch(slug, &c.market.condition_id, &c.market.question)?;
        tracing::info!(slug = %slug, "added to the watchlist");
        let r = self.db.recommendation(slug)?.context("the decision was not logged")?;
        self.hub.emit(Event::Decision { recommendation: r.clone() });
        Ok(r)
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
                end_date: p.end_date.map(|d| d.to_string()),
                image: Some(p.icon).filter(|url| !url.is_empty()),
            })
            .collect();
        let open_orders = ex
            .open_orders()
            .await?
            .into_iter()
            .map(|o| {
                let market = o.market.to_string();
                let known = self.db.order_market(&o.id, &market).unwrap_or_else(|e| {
                    tracing::warn!("looking up the market of order {}: {e:#}", o.id);
                    None
                });
                let (slug, title) = known.unzip();
                OpenOrder {
                    id: o.id,
                    market,
                    slug,
                    title: title.flatten(),
                    side: o.side.to_string(),
                    outcome: o.outcome,
                    price: d(o.price),
                    size: d(o.original_size),
                    matched: d(o.size_matched),
                    status: o.status.to_string(),
                    created_at: o.created_at.timestamp() as f64,
                }
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

fn exit_event(c: &Candidate, e: &Exit, placed: &Placed, dry_run: bool) -> OrderEvent {
    OrderEvent {
        ts: now(),
        slug: c.market.slug.clone(),
        title: c.market.question.clone(),
        side: "SELL".into(),
        outcome: e.outcome.to_string(),
        price: e.price,
        size: e.size,
        usd: e.usd,
        status: placed.status.clone(),
        dry_run,
        order_id: placed.order_id.clone(),
        message: placed.message.clone(),
        manual: false,
    }
}

fn order_event(c: &Candidate, t: &Trade, placed: &Placed, dry_run: bool, manual: bool) -> OrderEvent {
    OrderEvent {
        ts: now(),
        slug: c.market.slug.clone(),
        title: c.market.question.clone(),
        side: "BUY".into(),
        outcome: t.outcome.to_string(),
        price: t.price,
        size: t.size,
        usd: t.usd,
        status: placed.status.clone(),
        dry_run,
        order_id: placed.order_id.clone(),
        message: placed.message.clone(),
        manual,
    }
}

/// Keep a timestamp stable between updates, so an unchanged schedule is not sent again.
fn steady(old: &mut Option<f64>, new: Option<f64>) {
    if new.is_none_or(|t| old.is_none_or(|o| (o - t).abs() > 1.0)) {
        *old = new;
    }
}

pub fn spend(p: &Pipeline<'_>) -> Spend {
    let r = p.researcher.as_ref();
    Spend {
        jev_calls: p.jev.calls.get(),
        jev_usd: p.jev.total_cost.get(),
        briefs: r.map_or(0, |r| r.calls.get()),
        research_usd: r.map_or(0.0, |r| r.total_cost.get()),
    }
}
