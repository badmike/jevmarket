//! The price watch: between research cycles, the stored Jev views are held against live order
//! books with the same `evaluate` a pass uses, and held positions against `evaluate_exit`.
//! OpenRouter is only called when a book gives a signal whose view needs renewing, so a tick
//! without signals costs nothing but free book requests. The engine schedules the ticks, fetches
//! the books and positions; see `docs/daemon.md`.

use std::collections::{HashMap, HashSet};

use anyhow::{Context as _, Result};
use serde_json::Value;

use super::api::{Spend, WatchItem, WatchPosition, WatchSide, WatchSignal};
use super::engine::spend;
use super::now;
use crate::config::Settings;
use crate::executor::{Executor, Placed};
use crate::markets::Candidate;
use crate::pipeline::{Assessment, Pipeline, Research, distrust, log};
use crate::research::Researcher;
use crate::signal::{
    Exit, Holding, JevView, Outcome, Skip, SkipCode, Trade, Verdict, evaluate, evaluate_exit, exit_trigger,
    trigger_price,
};
use crate::store::{DecisionRow, Store};

/// The smallest midpoint move that sends a signal back to Jev: one tick.
const PRICE_MOVE: f64 = 0.01;

/// A stored Jev view, as [`super::db::Db::watch_views`] reads it.
#[derive(Debug, Clone)]
pub struct View {
    pub slug: String,
    pub question: String,
    pub condition_id: String,
    /// When Jev gave the view.
    pub ts: f64,
    /// The stored answers; the cost is zero, it was paid for back then.
    pub jev: JevView,
    /// The midpoint when Jev gave the view.
    pub midpoint: Option<f64>,
    /// The state Jev saw.
    pub state: Value,
    /// When the brief behind the view was written; `None` without one.
    pub brief_at: Option<f64>,
    /// A person put the market on the watchlist.
    pub pinned: bool,
}

/// A position the wallet holds, with Jev's latest view of its market.
#[derive(Debug, Clone)]
pub struct Held {
    pub view: View,
    pub holding: Holding,
}

/// What the watch shares with the research cycle. Reset when a cycle starts.
#[derive(Debug, Default)]
pub struct Window {
    /// Orders since the cycle started, against `max_trades_per_run`.
    pub trades: u32,
    /// Briefs since the cycle started, against `max_research_per_run`.
    pub briefs: u32,
    /// Markets the wallet holds a position or open order in, as the watch last read them.
    pub exposed: HashSet<String>,
}

/// A market whose live book gives a trade on its stored view.
pub struct Signal<'v> {
    pub view: &'v View,
    pub c: Candidate,
    pub trade: Trade,
}

/// A holding whose live book gives an exit on its stored view.
pub struct ExitSignal<'v> {
    pub held: &'v Held,
    pub c: Candidate,
}

/// What the watch did about a signal.
pub enum Acted {
    /// Nothing logged or spent: the executor would refuse the order, or the research budget of
    /// this cycle is used up. Holds the reason.
    Passed(String),
    /// Logged. `placed` is set when the verdict was still a trade.
    Decided { a: Box<Assessment>, verdict: Verdict, placed: Option<Placed> },
    /// Logged. `placed` is set when the exit still held on the renewed view; else why to hold.
    Exited { a: Box<Assessment>, exit: Result<Exit, String>, placed: Option<Placed> },
}

/// What a tick found and did.
pub struct Tick {
    /// Closest to a signal first.
    pub items: Vec<WatchItem>,
    pub signals: Vec<WatchSignal>,
    pub decisions: usize,
    pub orders: usize,
    pub spend: Spend,
}

/// One tick on the views, the holdings and their markets' live books: evaluate each, and with
/// `acting` act on the signals one at a time, exits first, like a pass places its orders. The
/// executor and OpenRouter client are only created when there is something to act on. `stop`
/// ends the tick before the next signal; `on` hears what was done about each, after it is logged.
#[expect(clippy::too_many_arguments, reason = "the engine's state, passed in so tests can fake it")]
pub async fn tick(
    views: &[View],
    held: &[Held],
    cands: &[Candidate],
    s: &Settings,
    store: &Store,
    window: &mut Window,
    acting: bool,
    stop: impl Fn() -> bool,
    mut on: impl FnMut(&Candidate, &Acted),
) -> Result<Tick> {
    let (mut items, signals) = check(views, cands, s);
    let (held_items, exits) = check_exits(held, cands, s);
    items.extend(held_items);
    sort(&mut items);
    let ts = now();
    let signal = |v: &View, sell| WatchSignal { ts, slug: v.slug.clone(), question: v.question.clone(), sell };
    let mut tick = Tick {
        items,
        signals: exits
            .iter()
            .map(|e| signal(&e.held.view, true))
            .chain(signals.iter().map(|g| signal(g.view, false)))
            .collect(),
        decisions: 0,
        orders: 0,
        spend: Spend::default(),
    };
    if !acting || tick.signals.is_empty() {
        return Ok(tick);
    }
    let mut ex = Executor::create(s, store, s.dry_run).await?;
    ex.trades_this_run = window.trades;
    window.exposed.clone_from(&ex.exposure(true).await?.condition_ids);
    let research = if s.research_enabled {
        Research::Budget(s.max_research_per_run.saturating_sub(window.briefs))
    } else {
        Research::Off
    };
    let pipeline = Pipeline::new(s, store, research);
    let result = async {
        let mut count = |acted: &Acted| match acted {
            Acted::Passed(_) => {}
            Acted::Decided { placed, .. } | Acted::Exited { placed, .. } => {
                tick.decisions += 1;
                tick.orders += usize::from(placed.as_ref().is_some_and(|p| p.ok));
            }
        };
        for e in &exits {
            if stop() {
                return anyhow::Ok(());
            }
            let acted = sell(e, pipeline.as_ref(), &mut ex, store, s).await?;
            count(&acted);
            on(&e.c, &acted);
        }
        for sig in &signals {
            if stop() {
                break;
            }
            let acted = act(sig, pipeline.as_ref(), &mut ex, store, s).await?;
            count(&acted);
            on(&sig.c, &acted);
        }
        anyhow::Ok(())
    }
    .await;
    window.trades = ex.trades_this_run;
    if let Ok(p) = &pipeline {
        tick.spend = spend(p);
        window.briefs += tick.spend.briefs;
    }
    result.map(|()| tick)
}

/// Evaluate every view against its market's live book, with the suspicious-edge guard of a pass:
/// the watched items and the trade signals.
pub fn check<'v>(views: &'v [View], cands: &[Candidate], s: &Settings) -> (Vec<WatchItem>, Vec<Signal<'v>>) {
    let by_id: HashMap<&str, &View> = views.iter().map(|v| (v.condition_id.as_str(), v)).collect();
    let mut items = Vec::with_capacity(views.len());
    let mut signals = Vec::new();
    for c in cands {
        let Some(&view) = by_id.get(c.market.condition_id.as_str()) else { continue };
        let verdict = distrust(evaluate(&view.jev, &c.book, s), s);
        // A view that fails the gates never trades, whatever the price.
        let gated = view.jev.answerable < s.min_answerable || view.jev.clarity < s.min_clarity;
        let side = |p: f64, ask: Option<f64>| {
            let trigger = trigger_price(p, c.book.tick_size, s).filter(|_| !gated);
            WatchSide { ask, trigger, distance: ask.zip(trigger).map(|(a, t)| round(a - t)) }
        };
        items.push(WatchItem {
            yes: side(view.jev.p_yes, c.book.yes_ask),
            no: side(1.0 - view.jev.p_yes, c.book.no_ask),
            signal: matches!(verdict, Verdict::Trade(_)),
            position: None,
            ..item(view, c)
        });
        if let Verdict::Trade(trade) = verdict {
            signals.push(Signal { view, c: c.clone(), trade });
        }
    }
    (items, signals)
}

/// Evaluate every holding against its market's live book: the watched items and the exit signals.
pub fn check_exits<'h>(held: &'h [Held], cands: &[Candidate], s: &Settings) -> (Vec<WatchItem>, Vec<ExitSignal<'h>>) {
    let by_id: HashMap<&str, &Candidate> = cands.iter().map(|c| (c.market.condition_id.as_str(), c)).collect();
    let mut items = Vec::with_capacity(held.len());
    let mut exits = Vec::new();
    for h in held {
        let Some(&c) = by_id.get(h.view.condition_id.as_str()) else { continue };
        let exit = evaluate_exit(&h.view.jev, &c.book, &h.holding, s);
        let (p, bid) = match h.holding.outcome {
            Outcome::Yes => (h.view.jev.p_yes, c.book.yes_bid),
            Outcome::No => (1.0 - h.view.jev.p_yes, c.book.no_bid),
        };
        let trigger = exit_trigger(p, h.holding.avg_price, c.book.tick_size, s);
        items.push(WatchItem {
            signal: exit.is_ok(),
            position: Some(WatchPosition {
                outcome: h.holding.outcome.as_str(),
                size: h.holding.size,
                avg_price: h.holding.avg_price,
                bid,
                trigger,
                distance: bid.zip(trigger).map(|(b, t)| round(t - b)),
            }),
            ..item(&h.view, c)
        });
        if exit.is_ok() {
            exits.push(ExitSignal { held: h, c: c.clone() });
        }
    }
    (items, exits)
}

/// A watched market without its sides.
fn item(view: &View, c: &Candidate) -> WatchItem {
    WatchItem {
        slug: view.slug.clone(),
        question: view.question.clone(),
        p_yes: view.jev.p_yes,
        view_at: view.ts,
        brief_at: view.brief_at,
        midpoint: c.book.midpoint(),
        end_date: c.market.end_date.map(|d| d.date_naive().to_string()),
        yes: WatchSide::default(),
        no: WatchSide::default(),
        signal: false,
        pinned: view.pinned,
        position: None,
        image: c.market.image.clone(),
    }
}

/// Signals first, then the least left to move. A side past its trigger without a signal (an ask
/// below the trade band, or a gap too big to trust) is not close to anything.
fn sort(items: &mut [WatchItem]) {
    let left = |i: &WatchItem| {
        let exit = i.position.and_then(|p| p.distance);
        [i.yes.distance, i.no.distance, exit].into_iter().flatten().filter(|d| *d > 0.0)
    };
    let closest = |i: &WatchItem| if i.signal { f64::NEG_INFINITY } else { left(i).fold(f64::INFINITY, f64::min) };
    items.sort_by(|a, b| closest(a).total_cmp(&closest(b)));
}

fn round(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

/// The view to act on: the stored one while its brief is younger than
/// `trade_brief_max_age_minutes` and the midpoint has not moved a tick (when Jev sees the price),
/// else a renewed one.
enum Fresh {
    Stored,
    Renewed(Box<Assessment>),
    /// Renewing failed; the skip to log.
    Failed(Skip),
    /// The research budget of this cycle is used up.
    NoBudget,
}

/// Renew the view when it needs it, see [`Fresh`]. `pipeline` holds the error when OpenRouter
/// cannot be used.
async fn freshen(
    view: &View,
    c: &Candidate,
    pipeline: Result<&Pipeline<'_>, &anyhow::Error>,
    store: &Store,
    s: &Settings,
) -> Fresh {
    let research = needs_research(view, s, now());
    if !(research || price_moved(view, c, s)) {
        return Fresh::Stored;
    }
    if research && pipeline.is_ok_and(|p| !p.researcher.as_ref().is_some_and(Researcher::budget_left)) {
        return Fresh::NoBudget;
    }
    let renewed = match pipeline {
        Ok(p) => renew(p, c, research, store, s).await,
        Err(e) => Err(anyhow::anyhow!("{e:#}")),
    };
    match renewed {
        Ok(a) => Fresh::Renewed(Box::new(a)),
        Err(e) => {
            let (code, what) = if research {
                (SkipCode::StaleBrief, "the brief could not be researched again before trading")
            } else {
                (SkipCode::StaleView, "Jev could not be asked again after the price moved")
            };
            tracing::warn!("{}: {what}: {e:#}", c.market.slug);
            Fresh::Failed(Skip { code, reason: format!("{what}: {e:#}") })
        }
    }
}

const NO_BUDGET: &str = "the research budget of this cycle is used up";

/// Act on one buy signal: renew the view if it needs it, evaluate again, place the order and log
/// the decision.
async fn act(
    sig: &Signal<'_>,
    pipeline: Result<&Pipeline<'_>, &anyhow::Error>,
    ex: &mut Executor<'_>,
    store: &Store,
    s: &Settings,
) -> Result<Acted> {
    let (view, c) = (sig.view, &sig.c);
    // Before anything is paid for or logged: the order would be refused anyway.
    if let Some(why) = ex.check(c, &sig.trade, false).await? {
        return Ok(Acted::Passed(why));
    }
    let (a, verdict) = match freshen(view, c, pipeline, store, s).await {
        Fresh::Stored => (stored(view), Verdict::Trade(sig.trade.clone())),
        Fresh::Renewed(a) => {
            let verdict = distrust(evaluate(&a.view, &c.book, s), s);
            (*a, verdict)
        }
        Fresh::Failed(skip) => (stored(view), Verdict::Skip(skip)),
        Fresh::NoBudget => return Ok(Acted::Passed(NO_BUDGET.into())),
    };
    let placed = match &verdict {
        Verdict::Trade(t) => Some(ex.place(c, t, false).await?),
        Verdict::Skip(_) => None,
    };
    log(store, c, &a, &verdict, placed.as_ref().is_some_and(|p| p.ok))?;
    Ok(Acted::Decided { a: Box::new(a), verdict, placed })
}

/// Act on one exit signal: renew the view if it needs it, and sell only if the exit still holds.
/// Logged as a `sell` decision, or `hold` when the renewed view says to keep the shares. A failed
/// renewal logs nothing and waits for the next tick.
async fn sell(
    sig: &ExitSignal<'_>,
    pipeline: Result<&Pipeline<'_>, &anyhow::Error>,
    ex: &mut Executor<'_>,
    store: &Store,
    s: &Settings,
) -> Result<Acted> {
    let (view, c) = (&sig.held.view, &sig.c);
    let a = match freshen(view, c, pipeline, store, s).await {
        Fresh::Stored => stored(view),
        Fresh::Renewed(a) => *a,
        Fresh::Failed(skip) => return Ok(Acted::Passed(skip.reason)),
        Fresh::NoBudget => return Ok(Acted::Passed(NO_BUDGET.into())),
    };
    let exit = evaluate_exit(&a.view, &c.book, &sig.held.holding, s);
    let placed = match &exit {
        Ok(e) => Some(ex.sell(c, e).await?),
        Err(_) => None,
    };
    let (action, edge, reason) = match (&exit, &placed) {
        (Ok(e), Some(p)) if p.ok => ("sell", Some(e.edge), e.rationale.as_str()),
        (Ok(e), _) => ("sell_unexecuted", Some(e.edge), e.rationale.as_str()),
        (Err(why), _) => ("hold", None, why.as_str()),
    };
    store.log_decision(&DecisionRow {
        slug: &c.market.slug,
        condition_id: &c.market.condition_id,
        question: &c.market.question,
        image: c.market.image.as_deref(),
        state: &a.state,
        p_yes: Some(a.view.p_yes),
        answerable: Some(a.view.answerable),
        clarity: a.view.clarity,
        yes_ask: c.book.yes_ask,
        no_ask: c.book.no_ask,
        midpoint: c.book.midpoint(),
        edge,
        action,
        skip_code: None,
        reason,
        jev_model: a.view.model.as_deref(),
        jev_cost: a.view.cost,
        research_cost: a.brief.as_ref().map(|b| b.cost),
        raw: &a.view.raw,
    })?;
    Ok(Acted::Exited { a: Box::new(a), exit, placed })
}

/// A new Jev view: on a fresh brief with `research`, else on the stored brief and the live price.
async fn renew(p: &Pipeline<'_>, c: &Candidate, research: bool, store: &Store, s: &Settings) -> Result<Assessment> {
    if research {
        let brief = p.brief(c, true).await?.0.context("the researcher returned no brief, see the log")?;
        return p.assess(c, Some(brief), false).await;
    }
    let brief = if s.research_enabled { store.get_brief(&c.market.slug, max_brief_age_s(s))? } else { None };
    p.assess(c, brief.map(|(b, _)| b), true).await
}

/// Research is on and the brief behind the view is missing or older than `trade_brief_max_age_minutes`.
fn needs_research(v: &View, s: &Settings, now: f64) -> bool {
    s.research_enabled && v.brief_at.is_none_or(|t| now - t > max_brief_age_s(s))
}

fn max_brief_age_s(s: &Settings) -> f64 {
    s.trade_brief_max_age_minutes as f64 * 60.0
}

/// Jev sees the price, and the midpoint moved at least a tick since the view.
fn price_moved(v: &View, c: &Candidate, s: &Settings) -> bool {
    s.jev_sees_market_price
        && match (v.midpoint, c.book.midpoint()) {
            (Some(then), Some(now)) => (now - then).abs() >= PRICE_MOVE - 1e-9,
            (None, now) => now.is_some(),
            (Some(_), None) => false,
        }
}

/// The stored view as an assessment, to log a trade on it. Nothing new was paid for.
fn stored(v: &View) -> Assessment {
    Assessment { state: v.state.clone(), brief: None, cached: true, view: v.jev.clone() }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::markets::test_candidate;

    /// Jev answers P(yes) 0.7 and is expected `jev` times; the researcher `briefs` times.
    async fn server(jev: u64, briefs: u64) -> MockServer {
        let server = MockServer::start().await;
        let answers = json!({"resolves_yes": {"noul": 0.7}, "answerable": {"noul": 0.9}, "clarity": {"score": 3}});
        Mock::given(path("/alpha/decisions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"answers": answers, "usage": {"cost": 0.00003}})),
            )
            .expect(jev)
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
        let s = Settings {
            openrouter_api_key: "k".into(),
            openrouter_base_url: server.uri(),
            dry_run: true,
            ..Settings::default()
        };
        (dir, store, s)
    }

    /// Market `m` with YES at 0.50 (midpoint 0.49).
    fn market() -> Candidate {
        let mut c = test_candidate(0.5, 10, 20_000.0);
        c.market.condition_id = "c".into();
        c
    }

    /// Jev's view of `m` at `midpoint`, on a brief written `brief_minutes` ago.
    fn view(p_yes: f64, midpoint: f64, brief_minutes: f64) -> View {
        View {
            slug: "m".into(),
            question: "Q?".into(),
            condition_id: "c".into(),
            ts: now() - 60.0,
            jev: JevView {
                p_yes,
                answerable: 0.9,
                clarity: 3,
                clarity_mean: None,
                clarity_confidence: None,
                model: None,
                cost: 0.0,
                raw: Value::Null,
            },
            midpoint: Some(midpoint),
            state: json!({"question": "Q?"}),
            brief_at: Some(now() - brief_minutes * 60.0),
            pinned: false,
        }
    }

    async fn run(views: &[View], s: &Settings, store: &Store, window: &mut Window) -> Tick {
        tick(views, &[], &[market()], s, store, window, true, || false, |_, _| {}).await.unwrap()
    }

    /// 10 YES shares of `m` bought at `avg_price`, with Jev's view at `p_yes`.
    fn held(p_yes: f64, avg_price: f64, brief_minutes: f64) -> Held {
        let holding = Holding { outcome: Outcome::Yes, token_id: market().book.yes_token_id, size: 10.0, avg_price };
        Held { view: view(p_yes, 0.49, brief_minutes), holding }
    }

    async fn exit(h: &Held, s: &Settings, store: &Store) -> Tick {
        tick(&[], std::slice::from_ref(h), &[market()], s, store, &mut Window::default(), true, || false, |_, _| {})
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_bid_above_jev_sells_a_profitable_holding() {
        let server = server(0, 0).await;
        let (_dir, store, s) = setup(&server);
        let t = exit(&held(0.40, 0.30, 10.0), &s, &store).await;
        assert!(t.signals[0].sell);
        assert_eq!((t.decisions, t.orders), (1, 1));
        assert!(store.has_sell_for("c", true).unwrap());
        let position = t.items[0].position.unwrap();
        assert_eq!((position.bid, position.trigger), (Some(0.48), Some(0.45)));
    }

    #[tokio::test]
    async fn a_holding_is_kept_while_jev_expects_more_or_the_profit_is_thin() {
        let server = server(0, 0).await;
        let (_dir, store, s) = setup(&server);
        for h in [held(0.50, 0.30, 10.0), held(0.40, 0.45, 10.0)] {
            let t = exit(&h, &s, &store).await;
            assert!(t.signals.is_empty() && !t.items[0].signal);
        }
        assert!(!store.has_sell_for("c", true).unwrap());
    }

    #[tokio::test]
    async fn an_exit_on_an_old_brief_is_checked_again_first() {
        // The fresh view says 0.7: the bid no longer beats it, so the shares are kept.
        let server = server(1, 1).await;
        let (_dir, store, s) = setup(&server);
        let t = exit(&held(0.40, 0.30, 180.0), &s, &store).await;
        assert_eq!((t.decisions, t.orders, t.spend.briefs), (1, 0, 1));
        assert_eq!(store.stats().unwrap().decisions, 1, "logged as a hold");
    }

    #[tokio::test]
    async fn a_paused_watch_only_looks() {
        let server = server(0, 0).await;
        let (_dir, store, s) = setup(&server);
        let t = tick(
            &[view(0.7, 0.49, 10.0)],
            &[],
            &[market()],
            &s,
            &store,
            &mut Window::default(),
            false,
            || false,
            |_, _| {},
        )
        .await
        .unwrap();
        assert_eq!((t.signals.len(), t.decisions), (1, 0));
        assert!(!store.has_order_for("c", true).unwrap());
    }

    #[tokio::test]
    async fn a_tick_without_signals_calls_nothing() {
        let server = server(0, 0).await;
        let (_dir, store, s) = setup(&server);
        let t = run(&[view(0.55, 0.49, 10.0)], &s, &store, &mut Window::default()).await;
        assert!(t.signals.is_empty());
        let yes = t.items[0].yes;
        assert_eq!((yes.trigger, yes.distance), (Some(0.47), Some(0.03)), "YES at 0.50 is 3 points above 0.47");
        assert_eq!(store.stats().unwrap().decisions, 0, "nothing logged");
    }

    #[tokio::test]
    async fn a_fresh_brief_trades_on_the_stored_view() {
        let server = server(0, 0).await;
        let (_dir, store, s) = setup(&server);
        let mut window = Window::default();
        let t = run(&[view(0.7, 0.49, 10.0)], &s, &store, &mut window).await;
        assert_eq!((t.signals.len(), t.decisions, t.orders, window.trades), (1, 1, 1, 1));
        assert!(store.has_order_for("c", true).unwrap());
    }

    #[tokio::test]
    async fn an_old_brief_is_researched_before_trading() {
        let server = server(1, 1).await;
        let (_dir, store, s) = setup(&server);
        let mut window = Window::default();
        let t = run(&[view(0.7, 0.49, 180.0)], &s, &store, &mut window).await;
        assert_eq!((t.orders, window.briefs, t.spend.jev_calls), (1, 1, 1));
    }

    #[tokio::test]
    async fn a_price_move_goes_back_to_jev() {
        let server = server(1, 0).await;
        let (_dir, store, s) = setup(&server);
        let t = run(&[view(0.7, 0.45, 10.0)], &s, &store, &mut Window::default()).await;
        assert_eq!((t.orders, t.spend.jev_calls, t.spend.briefs), (1, 1, 0));
    }

    #[tokio::test]
    async fn the_trade_cap_is_shared_with_the_cycle() {
        let server = server(0, 0).await;
        let (_dir, store, s) = setup(&server);
        let mut window = Window { trades: s.max_trades_per_run, ..Window::default() };
        let t = run(&[view(0.7, 0.49, 180.0)], &s, &store, &mut window).await;
        assert_eq!((t.signals.len(), t.decisions, t.orders), (1, 0, 0), "refused before research or logging");
    }

    #[tokio::test]
    async fn a_signal_without_openrouter_is_logged_as_a_skip() {
        let server = server(0, 0).await;
        let (_dir, store, s) = setup(&server);
        let s = Settings { openrouter_api_key: String::new(), ..s };
        let t = run(&[view(0.7, 0.49, 180.0)], &s, &store, &mut Window::default()).await;
        assert_eq!((t.decisions, t.orders), (1, 0));
        assert!(!store.has_order_for("c", true).unwrap());
    }
}
