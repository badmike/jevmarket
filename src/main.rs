//! jevmarket: a Polymarket trading bot priced by Jev (TypeSafe AI) via OpenRouter.

mod config;
mod daemon;
mod executor;
mod jev;
mod markets;
mod onboard;
mod openrouter;
mod pipeline;
mod research;
mod signal;
mod store;
mod ui;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use console::style;
use indexmap::IndexMap;
use polymarket_client_sdk_v2::{clob, gamma};
use rust_decimal::prelude::ToPrimitive as _;
use serde_json::json;
use tracing_subscriber::EnvFilter;

use crate::config::{Paths, Settings};
use crate::executor::Executor;
use crate::jev::{JevClient, Question};
use crate::markets::{load_candidate, scan, today, update_resolutions};
use crate::openrouter::OpenRouter;
use crate::pipeline::{Decided, Pipeline, Research, Step, is_fatal};
use crate::store::Store;

#[derive(Parser)]
#[command(name = "jevmarket", version, about = "Polymarket trading bot priced by Jev (TypeSafe AI) via OpenRouter.")]
#[command(arg_required_else_help = true)]
struct Cli {
    /// Config file to use instead of the OS default.
    #[arg(long, global = true, env = "JEVMARKET_CONFIG", value_name = "PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Guided first-time setup: OpenRouter key, models, Polymarket wallet, risk limits.
    Init,
    /// One hard-coded Jev call through OpenRouter to verify the key and response schema.
    JevTest {
        /// Print the raw JSON response only.
        #[arg(long)]
        raw: bool,
    },
    /// List candidate markets that pass the static filters, with live best bid/ask.
    Scan {
        #[arg(short = 'n', long, default_value_t = 15)]
        limit: usize,
        /// Market pages (50 each) to walk.
        #[arg(long, default_value_t = 5)]
        pages: u32,
    },
    /// Run only the researcher for one market and print the evidence brief.
    Research {
        /// Market slug or polymarket.com URL.
        reference: String,
        /// Ignore the cache and research again.
        #[arg(long)]
        fresh: bool,
    },
    /// Research + ask Jev about one market and show the proposed trade. Never places orders.
    Decide {
        /// Market slug or polymarket.com URL.
        reference: String,
        /// Print the state sent to Jev.
        #[arg(long)]
        show_state: bool,
        /// Skip the researcher step.
        #[arg(long)]
        no_research: bool,
        /// Ignore the research cache.
        #[arg(long)]
        fresh: bool,
    },
    /// Scan -> research -> ask Jev -> trade edges above threshold, under hard caps. Live by default.
    Run {
        /// Evaluate and log, but place no orders.
        #[arg(long)]
        dry_run: bool,
        /// Override max_trades_per_run.
        #[arg(long, value_name = "N")]
        max_trades: Option<u32>,
        /// Max candidate markets to evaluate.
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
        /// Repeat every N seconds until Ctrl-C.
        #[arg(long = "loop", value_name = "SECONDS")]
        every: Option<u64>,
        /// Skip the researcher step (expect near-zero trades).
        #[arg(long)]
        no_research: bool,
    },
    /// One-time on-chain approvals so the exchange can move your pUSD and outcome tokens.
    Setup,
    /// Show wallet balance, open positions and open orders.
    Positions,
    /// Decision/order counts, spend, Jev-vs-market calibration on resolved markets, and PnL.
    Stats,
    /// Fetch outcomes of decided or ordered markets that have resolved since the last check.
    Resolve,
    /// Run the trading loop in the background and serve a live web console for it.
    Daemon(daemon::Args),
    /// Inspect or edit the config file.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Print the config file and database locations.
    Path,
    /// Write a config file with every default.
    Init {
        /// Overwrite an existing file.
        #[arg(long)]
        force: bool,
    },
    /// Print the effective settings (file + environment), secrets masked.
    Show,
    /// Set one key, e.g. `config set min_edge 0.1`. Values are parsed as JSON when possible.
    Set { key: String, value: String },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{} {}", style("error:").red().bold(), chain(&e));
            ExitCode::FAILURE
        }
    }
}

/// The error and its causes on one line, skipping causes a wrapper already printed.
fn chain(e: &anyhow::Error) -> String {
    let mut out = String::new();
    for cause in e.chain().map(ToString::to_string) {
        if !out.contains(&cause) {
            if !out.is_empty() {
                out.push_str(": ");
            }
            out.push_str(&cause);
        }
    }
    out
}

async fn dispatch(cli: Cli) -> Result<()> {
    let paths = Paths::resolve(cli.config)?;
    match cli.command {
        Command::Config { action } => return config_cmd(&paths, action),
        Command::Init => return onboard::run(&paths).await,
        Command::Daemon(args) => return daemon::run(paths, args).await,
        _ => {}
    }
    let s = Settings::load(&paths.config)?;
    init_logging(&s);
    match cli.command {
        Command::JevTest { raw } => jev_test(&s, raw).await,
        Command::Scan { limit, pages } => scan_cmd(&s, limit, pages).await,
        Command::Research { reference, fresh } => research_cmd(&s, &paths, &reference, fresh).await,
        Command::Decide { reference, show_state, no_research, fresh } => {
            decide(&s, &paths, &reference, show_state, no_research, fresh).await
        }
        Command::Run { dry_run, max_trades, limit, every, no_research } => {
            let s = Settings {
                dry_run: dry_run || s.dry_run,
                max_trades_per_run: max_trades.unwrap_or(s.max_trades_per_run),
                ..s
            };
            run(&s, &paths, limit, every, no_research).await
        }
        Command::Setup => setup(&s, &paths).await,
        Command::Positions => positions(&s, &paths).await,
        Command::Stats => stats(&s, &paths),
        Command::Resolve => {
            let n = update_resolutions(&gamma::Client::default(), &open_store(&s, &paths)?).await?;
            println!("{n} new resolutions");
            Ok(())
        }
        Command::Config { .. } | Command::Init | Command::Daemon(_) => unreachable!("handled above"),
    }
}

fn init_logging(s: &Settings) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(format!("warn,jevmarket={}", s.log_level)));
    tracing_subscriber::fmt().with_env_filter(filter).with_target(false).with_writer(std::io::stderr).init();
}

fn clients(s: &Settings) -> Result<(gamma::Client, clob::Client)> {
    Ok((gamma::Client::default(), clob::Client::new(&s.clob_host, clob::Config::default())?))
}

fn open_store(s: &Settings, paths: &Paths) -> Result<Store> {
    Store::open(&s.db_path(paths))
}

fn config_cmd(paths: &Paths, action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Path => {
            let s = Settings::load(&paths.config)?;
            let exists = if paths.config.exists() { "" } else { " (not created yet)" };
            println!("config  {}{exists}", paths.config.display());
            println!("db      {}", s.db_path(paths).display());
        }
        ConfigAction::Init { force } => {
            config::init(&paths.config, force)?;
            println!("wrote {}", paths.config.display());
        }
        ConfigAction::Show => {
            let s = Settings::load(&paths.config)?;
            println!("{}", serde_json::to_string_pretty(&s.redacted())?);
        }
        ConfigAction::Set { key, value } => {
            config::set(&paths.config, &key, &value)?;
            println!("{key} updated in {}", paths.config.display());
        }
    }
    Ok(())
}

async fn jev_test(s: &Settings, raw: bool) -> Result<()> {
    let jev = JevClient::new(OpenRouter::new(s.require_openrouter_key()?, &s.openrouter_base_url), &s.jev_model);
    let state = json!({
        "question": "Will the sun rise in the east tomorrow?",
        "description": "Resolves YES if the sun rises in the east on the day after 'today'.",
        "today": today(),
        "days_until_resolution": 1,
    });
    let questions = IndexMap::from([
        ("resolves_yes", Question::noul("This market will resolve YES.")),
        (
            "domain",
            Question::choice(
                "What domain is this question about?",
                [
                    ("astronomy", "Celestial mechanics, planets, stars"),
                    ("politics", "Elections, governments, policy"),
                    ("sports", "Games, matches, athletes"),
                ],
            ),
        ),
        (
            "clarity",
            Question::score(
                "How clear are the resolution criteria?",
                ["Completely ambiguous", "Somewhat ambiguous", "Mostly clear", "Precise and objective"],
            ),
        ),
    ]);
    let body = jev.decide_raw(&state, &questions).await?;
    println!("{}", serde_json::to_string_pretty(&body)?);
    if raw {
        return Ok(());
    }
    let d = jev::parse(&body)?;
    let (p, domain, clarity) = (d.answer("resolves_yes"), d.answer("domain"), d.answer("clarity"));
    println!(
        "\n{} p_yes={:?} domain={:?} (conf {:?}) clarity={:?} | model={} cost=${:.6}",
        style("parsed:").bold(),
        p.noul(),
        domain.choice(),
        domain.confidence(),
        clarity.score_mean(),
        d.model.as_deref().unwrap_or("?"),
        d.usage.cost
    );
    Ok(())
}

async fn scan_cmd(s: &Settings, limit: usize, pages: u32) -> Result<()> {
    let (gamma, clob) = clients(s)?;
    let cands = scan(&gamma, &clob, s, limit, pages).await?;
    let mut t = ui::table(
        &format!("{} candidates", cands.len()),
        &["slug", "yes bid", "yes ask", "no ask", "days", "liq $", "vol $"],
        1,
    );
    for c in &cands {
        let (m, b) = (&c.market, &c.book);
        t.add_row([
            m.slug.chars().take(70).collect(),
            ui::price(b.yes_bid),
            ui::price(b.yes_ask),
            ui::price(b.no_ask),
            m.days_to_resolution().map_or_else(|| "?".into(), |d| d.to_string()),
            ui::thousands(m.liquidity),
            ui::thousands(m.volume),
        ]);
    }
    println!("{t}");
    Ok(())
}

async fn research_cmd(s: &Settings, paths: &Paths, reference: &str, fresh: bool) -> Result<()> {
    let store = open_store(s, paths)?;
    let pipeline = Pipeline::new(s, &store, Research::Unlimited)?;
    let (gamma, clob) = clients(s)?;
    let cand = load_candidate(&gamma, &clob, reference).await?;
    let (Some(brief), cached) = pipeline.brief(&cand, fresh).await? else {
        bail!("no brief produced");
    };
    println!("{}", style(&cand.market.question).bold());
    ui::print_brief(&brief, cached, true);
    Ok(())
}

async fn decide(
    s: &Settings,
    paths: &Paths,
    reference: &str,
    show_state: bool,
    no_research: bool,
    fresh: bool,
) -> Result<()> {
    let store = open_store(s, paths)?;
    let research = if no_research || !s.research_enabled { Research::Off } else { Research::Unlimited };
    let pipeline = Pipeline::new(s, &store, research)?;
    let (gamma, clob) = clients(s)?;
    let cand = load_candidate(&gamma, &clob, reference).await?;
    let (a, verdict) = match pipeline.decide(&cand, fresh).await? {
        Decided::Unclear(reason) => {
            ui::print_unclear(&cand, &reason);
            return Ok(());
        }
        Decided::Assessed(a, verdict) => (a, verdict),
    };
    if show_state {
        println!("{}", serde_json::to_string_pretty(&a.state)?);
    }
    ui::print_decision(&cand, &a, &verdict);
    pipeline.log(&cand, &a, &verdict, false)
}

async fn run(s: &Settings, paths: &Paths, limit: usize, every: Option<u64>, no_research: bool) -> Result<()> {
    let store = open_store(s, paths)?;
    let research =
        if no_research || !s.research_enabled { Research::Off } else { Research::Budget(s.max_research_per_run) };
    let pipeline = Pipeline::new(s, &store, research)?;
    let mut ex = Executor::create(s, &store, s.dry_run).await?;
    let (gamma, clob) = clients(s)?;
    loop {
        let pass = run_pass(s, &pipeline, &mut ex, &gamma, &clob, limit).await;
        match (pass, every) {
            (Ok(()), None) => return Ok(()),
            (Err(e), None) => return Err(e),
            (Err(e), Some(_)) if is_fatal(&e) => return Err(e),
            (Err(e), Some(_)) => eprintln!("{} {}", style("pass failed:").red(), chain(&e)),
            (Ok(()), Some(_)) => {}
        }
        let secs = every.unwrap_or_default();
        tokio::select! {
            () = tokio::time::sleep(Duration::from_secs(secs)) => {}
            _ = tokio::signal::ctrl_c() => {
                println!("stopped");
                return Ok(());
            }
        }
    }
}

/// One `run` pass, printed to the terminal.
async fn run_pass(
    s: &Settings,
    pipeline: &Pipeline<'_>,
    ex: &mut Executor<'_>,
    gamma: &gamma::Client,
    clob: &clob::Client,
    limit: usize,
) -> Result<()> {
    let dry_run = s.dry_run;
    pipeline
        .pass(ex, gamma, clob, limit, std::future::pending(), |step| match step {
            Step::Resolved(n) => println!("{}", style(format!("{n} markets resolved since the last pass")).dim()),
            Step::AlreadyExposed(c) => println!("{}", style(format!("{}: already exposed, skip", c.market.slug)).dim()),
            Step::Scanned { candidates, exposure_usd } => {
                ui::rule(&format!("{candidates} candidates | exposure ${exposure_usd:.2} | dry_run={dry_run}"));
            }
            Step::Unclear(c, reason) => ui::print_unclear(c, reason),
            Step::Failed(c, e) => println!("{}", style(format!("{}: {}", c.market.slug, chain(e))).red()),
            Step::Decided { c, a, verdict, placed } => {
                ui::print_decision(c, a, verdict);
                match placed {
                    Some(p) if p.ok => {
                        println!("  {} order_id={}", style(&p.status).green(), p.order_id.as_deref().unwrap_or("-"));
                    }
                    Some(p) => {
                        println!(
                            "  {}",
                            style(format!("{}: {}", p.status, p.message.as_deref().unwrap_or_default())).yellow()
                        );
                    }
                    None => {}
                }
            }
            Step::TradeCapReached => println!("{}", style("trade cap for this run reached").bold()),
            Step::Stopped => {}
        })
        .await?;
    println!("{}", style(pipeline.spend()).dim());
    Ok(())
}

async fn setup(s: &Settings, paths: &Paths) -> Result<()> {
    let store = open_store(s, paths)?;
    let ex = Executor::create(s, &store, false).await?;
    println!("wallet {} ({})", ex.wallet_label(), ex.wallet_type());
    println!("{}", ex.setup_approvals(6).await?);
    Ok(())
}

async fn positions(s: &Settings, paths: &Paths) -> Result<()> {
    let store = open_store(s, paths)?;
    let mut ex = Executor::create(s, &store, true).await?;
    let balance = match ex.collateral_balance_usd().await {
        Some(b) => format!("${b:.2}"),
        None if ex.authenticated() => "n/a".into(),
        None => "n/a (no key)".into(),
    };
    println!("wallet {} ({})  pUSD: {balance}", ex.wallet_label(), ex.wallet_type());

    let d = |x: rust_decimal::Decimal, dp: usize| format!("{:.dp$}", x.to_f64().unwrap_or_default());
    let mut t = ui::table("open positions", &["slug", "outcome", "size", "avg", "cur", "value $", "pnl $"], 2);
    for p in ex.positions().await? {
        t.add_row([
            p.slug.chars().take(60).collect(),
            p.outcome.clone(),
            d(p.size, 2),
            d(p.avg_price, 3),
            d(p.cur_price, 3),
            d(p.current_value, 2),
            format!("{:+.2}", p.cash_pnl.to_f64().unwrap_or_default()),
        ]);
    }
    println!("{t}");

    let mut t = ui::table("open orders", &["id", "side", "outcome", "price", "size", "matched", "status"], 3);
    for o in ex.open_orders().await? {
        t.add_row([
            o.id.chars().take(12).collect(),
            o.side.to_string(),
            o.outcome.clone(),
            o.price.to_string(),
            o.original_size.to_string(),
            o.size_matched.to_string(),
            o.status.to_string(),
        ]);
    }
    println!("{t}");

    let exposure = ex.exposure(true).await?;
    println!(
        "exposure: positions ${:.2} + open orders ${:.2} = ${:.2} (cap ${:.2})",
        exposure.positions_usd,
        exposure.open_orders_usd,
        exposure.total(),
        s.max_open_exposure_usd
    );
    Ok(())
}

fn stats(s: &Settings, paths: &Paths) -> Result<()> {
    let st = open_store(s, paths)?.stats()?;
    println!(
        "decisions={} trade_signals={} jev_cost=${:.5} briefs={} research_cost=${:.4} live_orders={} live_usd=${:.2}",
        st.decisions, st.trade_signals, st.jev_cost_usd, st.briefs, st.research_cost_usd, st.live_orders, st.live_usd
    );
    let mut t = ui::table("Jev P(yes) buckets vs market midpoint", &["bucket", "n", "avg jev", "avg market"], 0);
    for b in &st.buckets {
        t.add_row([
            format!("{:.1}-{:.1}", b.bucket as f64 / 10.0, (b.bucket + 1) as f64 / 10.0),
            b.n.to_string(),
            format!("{:.2}", b.avg_p),
            format!("{:.2}", b.avg_market),
        ]);
    }
    println!("{t}");

    let c = &st.calibration;
    let mut t = ui::table(
        &format!("Jev vs market on {} resolved markets (latest decision each)", c.all.n),
        &["by", "group", "n", "hit rate", "brier jev", "brier market"],
        2,
    );
    let sections = [
        ("all", std::slice::from_ref(&c.all)),
        ("variant", &c.by_variant[..]),
        ("edge", &c.by_edge[..]),
        ("answerable", &c.by_answerable[..]),
        ("clarity", &c.by_clarity[..]),
    ];
    for (by, groups) in sections {
        for g in groups.iter().filter(|g| g.n > 0) {
            t.add_row([
                by.to_owned(),
                g.label.clone(),
                g.n.to_string(),
                format!("{:.0}%", g.hit_rate * 100.0),
                format!("{:.3}", g.brier_jev),
                format!("{:.3}", g.brier_market),
            ]);
        }
    }
    println!("{t}");
    for (name, p) in [("live", &st.live_pnl), ("dry-run", &st.dry_run_pnl)] {
        println!(
            "{name} pnl: {} of {} orders resolved, staked ${:.2}, paid out ${:.2}, pnl ${:+.2}",
            p.resolved, p.orders, p.staked_usd, p.payout_usd, p.pnl_usd
        );
    }
    Ok(())
}
