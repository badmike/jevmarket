//! `jevmarket daemon`: the trading loop as a long-running process, with a live web console.
//!
//! The engine thread owns the pipeline and runs the passes (see [`engine`]); the HTTP server
//! answers snapshot queries from its own read connection, forwards commands to the engine and
//! streams [`api::Event`]s over SSE. See `docs/daemon.md`.

mod api;
mod assets;
mod db;
mod engine;
mod http;
mod hub;

use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, anyhow, bail};
use tokio::sync::{mpsc, watch};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

use self::api::{LoopState, Status};
use self::db::Db;
use self::engine::Engine;
use self::hub::{Hub, LogLayer};
use crate::config::{Paths, Settings};

#[derive(clap::Args)]
pub struct Args {
    /// Address to listen on. Keep it on localhost and put a TLS proxy in front for remote access.
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: SocketAddr,
    /// Seconds between passes.
    #[arg(long = "loop", value_name = "SECONDS", default_value_t = 900, value_parser = clap::value_parser!(u64).range(1..))]
    every: u64,
    /// Evaluate and log, but place no orders, whatever the config says.
    #[arg(long)]
    dry_run: bool,
    /// Max candidate markets per pass.
    #[arg(short = 'n', long, default_value_t = 20)]
    limit: usize,
    /// URL prefix when a reverse proxy serves the console under a sub-path, e.g. `/jevmarket`.
    #[arg(long, value_name = "PATH", default_value = "", value_parser = parse_base_path)]
    base_path: String,
}

/// What the command line fixed for the daemon's lifetime.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub dry_run_forced: bool,
    pub loop_secs: u64,
    pub limit: usize,
}

pub async fn run(paths: Paths, args: Args) -> Result<()> {
    let opts = Options { dry_run_forced: args.dry_run, loop_secs: args.every, limit: args.limit };
    let s = settings(&paths, opts.dry_run_forced)?;
    let db_path = s.db_path(&paths);
    let db = Db::open(&db_path)?;
    let hub = Hub::new(Status {
        version: env!("CARGO_PKG_VERSION"),
        build: assets::build_id(),
        started_at: now(),
        state: LoopState::Waiting,
        loop_secs: opts.loop_secs,
        next_pass_at: None,
        dry_run: s.dry_run,
        dry_run_forced: opts.dry_run_forced,
        pass: None,
        last_pass: None,
        spend_total: db.store.spend()?,
        last_error: None,
    });
    init_logging(&s, hub.clone());

    let (commands, command_rx) = mpsc::channel(32);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let engine = Engine::new(paths.clone(), opts, hub.clone(), db);
    let engine_rx = shutdown_rx.clone();
    let engine = std::thread::Builder::new().name("engine".into()).spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(engine.run(command_rx, engine_rx));
        anyhow::Ok(())
    })?;

    let state = http::AppState {
        paths,
        opts,
        hub,
        db: Mutex::new(Db::open(&db_path)?),
        commands,
        shutdown: shutdown_rx,
        index_html: assets::index_html(&args.base_path),
    };
    let listener = tokio::net::TcpListener::bind(args.bind).await.with_context(|| format!("binding {}", args.bind))?;
    tracing::info!("console on http://{}{}/", listener.local_addr()?, args.base_path);
    axum::serve(listener, http::router(state, &args.base_path))
        .with_graceful_shutdown(async move {
            signal().await;
            tracing::info!("shutting down");
            shutdown.send_replace(true);
        })
        .await?;
    engine.join().map_err(|_| anyhow!("the engine thread panicked"))?
}

/// Settings as the engine uses them: the config file, with `--dry-run` forcing dry runs.
pub fn settings(paths: &Paths, dry_run_forced: bool) -> Result<Settings> {
    let s = Settings::load(&paths.config)?;
    Ok(Settings { dry_run: s.dry_run || dry_run_forced, ..s })
}

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// Terminal output as for every other command, plus log lines on the event stream.
fn init_logging(s: &Settings, hub: std::sync::Arc<Hub>) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(format!("warn,jevmarket={}", s.log_level)));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(false).with_writer(std::io::stderr))
        .with(LogLayer(hub))
        .init();
}

async fn signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            },
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

/// `/jevmarket` from `jevmarket`, `/jevmarket/` or `/jevmarket`; empty for the root.
fn parse_base_path(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_matches('/');
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if !trimmed
        .split('/')
        .all(|seg| !seg.is_empty() && seg.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)))
    {
        bail!("use letters, digits, `-`, `_` and `.` between slashes");
    }
    Ok(format!("/{trimmed}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_paths_are_normalized() {
        assert_eq!(parse_base_path("").unwrap(), "");
        assert_eq!(parse_base_path("/").unwrap(), "");
        assert_eq!(parse_base_path("jevmarket/").unwrap(), "/jevmarket");
        assert_eq!(parse_base_path("/tools/jev").unwrap(), "/tools/jev");
        assert!(parse_base_path("/a b").is_err());
        assert!(parse_base_path("/a//b").is_err());
    }
}
