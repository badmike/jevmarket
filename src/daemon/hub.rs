//! The daemon's shared state: the event bus, the loop status, the recent-activity buffer and the
//! price watch's last snapshot.
//! Written by the engine thread and the log layer, read by HTTP handlers.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::{Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

use super::api::{Event, LogLevel, Status, WatchItem};
use super::now;

/// Events a slow SSE client may fall behind by before it is told to resync.
const BUS_CAPACITY: usize = 512;
const RECENT: usize = 200;

pub struct Hub {
    tx: broadcast::Sender<Event>,
    status: Mutex<Status>,
    recent: Mutex<VecDeque<Event>>,
    watchlist: Mutex<Vec<WatchItem>>,
}

impl Hub {
    pub fn new(status: Status) -> Arc<Self> {
        Arc::new(Self {
            tx: broadcast::channel(BUS_CAPACITY).0,
            status: Mutex::new(status),
            recent: Mutex::new(VecDeque::with_capacity(RECENT)),
            watchlist: Mutex::new(Vec::new()),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    pub fn emit(&self, event: Event) {
        if event.is_activity() {
            let mut recent = lock(&self.recent);
            if recent.len() == RECENT {
                recent.pop_front();
            }
            recent.push_back(event.clone());
        }
        // No receivers is fine: nobody has the console open.
        let _ = self.tx.send(event);
    }

    pub fn status(&self) -> Status {
        lock(&self.status).clone()
    }

    /// Change the status and broadcast it, if anything changed.
    pub fn update(&self, change: impl FnOnce(&mut Status)) {
        let status = {
            let mut s = lock(&self.status);
            let before = s.clone();
            change(&mut s);
            if *s == before {
                return;
            }
            s.clone()
        };
        self.emit(Event::Status { status });
    }

    /// The watched markets as the last price-watch tick saw them.
    pub fn watchlist(&self) -> Vec<WatchItem> {
        lock(&self.watchlist).clone()
    }

    pub fn set_watchlist(&self, items: Vec<WatchItem>) {
        *lock(&self.watchlist) = items;
    }

    /// Show a market as off the watchlist until the next tick decides whether it is still watched.
    pub fn unpin(&self, slug: &str) {
        lock(&self.watchlist).iter_mut().filter(|i| i.slug == slug).for_each(|i| i.pinned = false);
    }

    /// Activity since the daemon started, oldest first.
    pub fn recent(&self) -> Vec<Event> {
        lock(&self.recent).iter().cloned().collect()
    }
}

/// A poisoned lock only means another thread panicked mid-update; the data is still usable.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Forwards jevmarket's own log lines at info and above to the event bus, except the pass
/// lines that events already cover.
pub struct LogLayer(pub Arc<Hub>);

impl<S: Subscriber> Layer<S> for LogLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        let meta = event.metadata();
        let level = match *meta.level() {
            Level::ERROR => LogLevel::Error,
            Level::WARN => LogLevel::Warn,
            Level::INFO => LogLevel::Info,
            _ => return,
        };
        if !meta.target().starts_with("jevmarket") || meta.target() == super::engine::PASS_LOG {
            return;
        }
        let mut line = Line::default();
        event.record(&mut line);
        self.0.emit(Event::Log { ts: now(), level, message: line.0 });
    }
}

/// `message key=value ...`, like the terminal output.
#[derive(Default)]
struct Line(String);

impl Visit for Line {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write as _;
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        let _ = if field.name() == "message" {
            write!(self.0, "{value:?}")
        } else {
            write!(self.0, "{}={value:?}", field.name())
        };
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_debug(field, &format_args!("{value}"));
    }
}
