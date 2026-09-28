//! `jevmarket init`: a guided first-time setup. Walks through OpenRouter, model choice,
//! Polymarket funding and risk limits, checks each credential live, and writes the config
//! only at the very end, so cancelling (Esc or Ctrl-C) leaves nothing half-configured.

use std::io::IsTerminal as _;
use std::str::FromStr as _;
use std::time::Duration;

use alloy::signers::local::PrivateKeySigner;
use anyhow::{Result, bail};
use console::style;
use indexmap::IndexMap;
use inquire::validator::Validation;
use inquire::{Confirm, CustomType, InquireError, Password, PasswordDisplayMode, Select, Text};
use polymarket_client_sdk_v2::clob;
use polymarket_client_sdk_v2::types::Address;
use serde_json::{Map, Value, json};

use crate::config::{self, Paths, Settings};
use crate::executor::Executor;
use crate::jev::{JevClient, Question};
use crate::openrouter::{OpenRouter, OpenRouterError, Policy};
use crate::store::Store;
use crate::ui;

const CHECK: Policy =
    Policy { service: "OpenRouter", timeout: Duration::from_secs(20), max_attempts: 1, base_delay: Duration::ZERO };

const JEV_MODELS: [(&str, &str); 2] = [
    ("typesafe/jev-1.13", "pinned version, behavior only changes when you change it (recommended)"),
    ("typesafe/jev-latest", "always the newest Jev; re-run `jevmarket jev-test` after updates"),
];

/// Researcher models the original project tested, cheapest strong option first.
const RESEARCH_MODELS: [(&str, &str); 5] = [
    ("deepseek/deepseek-v4-pro-0813", "default, cheap and strong"),
    ("z-ai/glm-5.3", "tested alternative"),
    ("qwen/qwen3.8-max-0902", "tested alternative"),
    ("moonshotai/kimi-k3", "tested alternative"),
    ("anthropic/claude-sonnet-5", "native web search, about 2x the cost"),
];

pub async fn run(paths: &Paths) -> Result<()> {
    if !std::io::stdin().is_terminal() {
        bail!("`jevmarket init` is interactive; in scripts use `jevmarket config set <key> <value>`");
    }
    match wizard(paths).await {
        Err(e) if is_cancel(&e) => {
            println!("\n{}", style("Setup cancelled. Nothing was saved.").yellow());
            Ok(())
        }
        other => other,
    }
}

fn is_cancel(e: &anyhow::Error) -> bool {
    matches!(e.downcast_ref(), Some(InquireError::OperationCanceled | InquireError::OperationInterrupted))
}

/// What the wizard collected, before it is written.
struct Answers {
    changes: Map<String, Value>,
    openrouter_key: String,
    private_key: Option<String>,
}

async fn wizard(paths: &Paths) -> Result<()> {
    let existing = paths.config.exists();
    let current = Settings::load(&paths.config)?;
    println!("{}", style("jevmarket setup").bold());
    println!(
        "Four short steps: OpenRouter, models, Polymarket, risk limits. Every credential is checked live.\n\
         Nothing is saved until you confirm at the end; Esc or Ctrl-C cancels."
    );
    if existing {
        println!("{}", style(format!("Updating {}; current values are the defaults.", paths.config.display())).dim());
    }

    let mut answers = Answers { changes: Map::new(), openrouter_key: String::new(), private_key: None };
    let api = openrouter_step(&current, &mut answers).await?;
    models_step(&current, &api, &mut answers).await?;
    polymarket_step(&current, &mut answers).await?;
    limits_step(&current, &mut answers)?;
    save_step(paths, answers)
}

// --- 1. OpenRouter -------------------------------------------------------------------------

async fn openrouter_step(current: &Settings, answers: &mut Answers) -> Result<OpenRouter> {
    section(1, "OpenRouter");
    println!("OpenRouter serves Jev (the pricing model) and the researcher (web search). One key covers both.");
    step(1, "Create an account", "https://openrouter.ai");
    step(
        2,
        "Add credits (Jev has no free tier; $5 lasts for thousands of decisions)",
        "https://openrouter.ai/settings/credits",
    );
    step(3, "Create an API key", "https://openrouter.ai/settings/keys");
    println!();

    loop {
        let mut prompt = Password::new("OpenRouter API key:")
            .with_display_mode(PasswordDisplayMode::Masked)
            .without_confirmation()
            .with_validator(|key: &str| {
                Ok(if key.is_empty() || key.starts_with("sk-or-") {
                    Validation::Valid
                } else {
                    Validation::Invalid("OpenRouter keys start with `sk-or-`".into())
                })
            });
        let masked = masked(&current.openrouter_api_key);
        let help = format!("Enter keeps the current key ({masked})");
        if !current.openrouter_api_key.is_empty() {
            prompt = prompt.with_help_message(&help);
        }
        let entered = prompt.prompt()?.trim().to_owned();
        let key = if entered.is_empty() { current.openrouter_api_key.clone() } else { entered };
        if key.is_empty() {
            println!("{}", style("A key is required to continue.").red());
            continue;
        }

        let api = OpenRouter::new(&key, &current.openrouter_base_url);
        match api.get("/v1/key", CHECK).await {
            Ok(info) => {
                let data = &info["data"];
                let label = data["label"].as_str().filter(|l| !l.is_empty()).unwrap_or("unnamed key");
                ok(&format!("key accepted ({label}, ${:.2} used so far)", data["usage"].as_f64().unwrap_or_default()));
                if data["is_free_tier"].as_bool() == Some(true) {
                    warn("this account has never bought credits: Jev calls fail with 402 until you add some");
                }
                if key != current.openrouter_api_key {
                    answers.changes.insert("openrouter_api_key".into(), json!(key));
                }
                answers.openrouter_key = key;
                return Ok(api);
            }
            Err(OpenRouterError::Status { status: 401 | 403, message, .. }) => {
                warn(&format!("OpenRouter rejected this key: {message}"));
                if !Confirm::new("Try another key?").with_default(true).prompt()? {
                    bail!("no working OpenRouter key");
                }
            }
            Err(e) => {
                warn(&format!("could not verify the key: {e}"));
                if Confirm::new("Use it anyway?").with_default(false).prompt()? {
                    answers.changes.insert("openrouter_api_key".into(), json!(key));
                    answers.openrouter_key = key;
                    return Ok(api);
                }
            }
        }
    }
}

// --- 2. Models -----------------------------------------------------------------------------

async fn models_step(current: &Settings, api: &OpenRouter, answers: &mut Answers) -> Result<()> {
    section(2, "Models");
    println!("Jev turns the evidence into a calibrated probability. It is not a chat model; its version matters.");
    let mut jev_options: Vec<String> =
        JEV_MODELS.iter().map(|(id, note)| format!("{id}  {}", style(note).dim())).collect();
    jev_options.push("Another version…".into());
    let start = JEV_MODELS.iter().position(|(id, _)| *id == current.jev_model).unwrap_or(JEV_MODELS.len());
    let pick = Select::new("Jev model:", jev_options).with_starting_cursor(start).raw_prompt()?;
    let jev_model = match JEV_MODELS.get(pick.index) {
        Some((id, _)) => (*id).to_owned(),
        None => Text::new("Jev model id:").with_default(&current.jev_model).prompt()?,
    };
    set_if_changed(answers, "jev_model", &current.jev_model, &jev_model);

    println!("\nThe researcher is a chat model with web search. It writes a dated evidence brief for every market,");
    println!("so it is where most of the money goes: about $0.01 to $0.02 per brief with the default.");
    let catalog = match api.get("/v1/models", CHECK).await {
        Ok(v) => catalog(&v),
        Err(e) => {
            warn(&format!("could not load the model list ({e}); showing the tested models without prices"));
            Vec::new()
        }
    };
    let research_model = pick_research_model(current, &catalog)?;
    set_if_changed(answers, "research_model", &current.research_model, &research_model);

    let cost = "one test decision, about $0.00003";
    if Confirm::new(&format!("Check that Jev answers with this key? ({cost})")).with_default(true).prompt()? {
        let jev = JevClient::new(api.clone(), &jev_model);
        let state = json!({"question": "Will the sun rise in the east tomorrow?", "days_until_resolution": 1});
        let questions = IndexMap::from([("resolves_yes", Question::noul("This market will resolve YES."))]);
        match jev.decide(&state, &questions).await {
            Ok((d, _)) => ok(&format!(
                "Jev answered: P(yes)={:.2}, model {}, cost ${:.6}",
                d.answer("resolves_yes").noul().unwrap_or_default(),
                d.model.as_deref().unwrap_or(&jev_model),
                d.usage.cost
            )),
            Err(OpenRouterError::Status { status: 402, .. }) => {
                warn("no credits: add some at https://openrouter.ai/settings/credits, then run `jevmarket jev-test`");
            }
            Err(e) => warn(&format!("Jev call failed: {e}. Check the model id, then run `jevmarket jev-test`")),
        }
    }
    Ok(())
}

/// A chat model from OpenRouter's catalog, with prices per million tokens.
struct Model {
    id: String,
    prompt_per_m: Option<f64>,
    completion_per_m: Option<f64>,
}

impl Model {
    fn price(&self) -> String {
        match (self.prompt_per_m, self.completion_per_m) {
            (Some(i), Some(o)) => format!("${i:.2} in / ${o:.2} out per 1M tokens"),
            _ => "price unknown".into(),
        }
    }
}

fn catalog(models: &Value) -> Vec<Model> {
    let per_m = |v: &Value| v.as_str().and_then(|p| p.parse::<f64>().ok()).map(|p| p * 1e6);
    let mut out: Vec<Model> = models["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["architecture"]["output_modalities"].as_array().is_none_or(|o| o.contains(&json!("text"))))
        .filter_map(|m| {
            Some(Model {
                id: m["id"].as_str()?.to_owned(),
                prompt_per_m: per_m(&m["pricing"]["prompt"]),
                completion_per_m: per_m(&m["pricing"]["completion"]),
            })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn pick_research_model(current: &Settings, catalog: &[Model]) -> Result<String> {
    let find = |id: &str| catalog.iter().find(|m| m.id == id);
    let mut ids: Vec<String> = RESEARCH_MODELS.iter().map(|(id, _)| (*id).to_owned()).collect();
    let mut options: Vec<String> = RESEARCH_MODELS
        .iter()
        .map(|(id, note)| {
            let price = find(id).map(Model::price).unwrap_or_default();
            format!("{id}  {}", style(format!("{note}  {price}")).dim())
        })
        .collect();
    if !ids.contains(&current.research_model) {
        options.insert(0, format!("{}  {}", current.research_model, style("current").dim()));
        ids.insert(0, current.research_model.clone());
    }
    if !catalog.is_empty() {
        options.push(format!("Browse all {} models…", catalog.len()));
    }
    options.push("Enter a model id…".into());

    let start = ids.iter().position(|id| *id == current.research_model).unwrap_or_default();
    let pick = Select::new("Researcher model:", options).with_starting_cursor(start).raw_prompt()?;
    if let Some(id) = ids.get(pick.index) {
        return Ok(id.clone());
    }
    if !catalog.is_empty() && pick.index == ids.len() {
        let all: Vec<String> = catalog.iter().map(|m| format!("{}  {}", m.id, style(m.price()).dim())).collect();
        let chosen = Select::new("Search models (type to filter):", all).with_page_size(12).raw_prompt()?;
        return Ok(catalog[chosen.index].id.clone());
    }
    Ok(Text::new("Model id (see https://openrouter.ai/models):").with_default(&current.research_model).prompt()?)
}

// --- 3. Polymarket -------------------------------------------------------------------------

async fn polymarket_step(current: &Settings, answers: &mut Answers) -> Result<()> {
    section(3, "Polymarket");
    println!("Polymarket settles in pUSD on Polygon. Use a dedicated wallet holding only what you can afford to lose:");
    println!("the limits you set in the next step are the only brake.");

    if let Ok(client) = clob::Client::new(&current.clob_host, clob::Config::default())
        && let Ok(geo) = client.check_geoblock().await
    {
        if geo.blocked {
            warn(&format!(
                "Polymarket blocks trading from your location ({} {}). You can still scan, research and dry-run.",
                geo.country, geo.region
            ));
        } else {
            ok(&format!("trading is available from your location ({})", geo.country));
        }
    }

    let options =
        vec!["Trade from my polymarket.com account (recommended)", "Not now: dry runs only, set up trading later"];
    if Select::new("How will you trade?", options).raw_prompt()?.index == 0 {
        step(1, "Sign up and deposit; deposits arrive as pUSD", "https://polymarket.com");
        step_text(
            2,
            "Export the private key of the account you log in with: email and social logins offer a key export \
             in Polymarket's settings; for MetaMask or another browser wallet, export it from that wallet app",
        );
        step(
            3,
            "Create builder API credentials under Settings, Builder",
            "https://polymarket.com/settings?tab=builder",
        );
        step_text(
            4,
            "After this setup, run `jevmarket setup`: it creates the deposit wallet Polymarket requires for API \
             trading, approves the exchange and shows where to send funds",
        );
        let key = ask_private_key(current)?;
        println!("  signer address: {}", signer(&key).address());
        answers.private_key = Some(key);
    } else {
        println!("Dry runs work without a key. A wallet address lets them respect your real exposure.");
        if let Some(wallet) = ask_wallet(current, false)? {
            answers.changes.insert("polymarket_deposit_wallet".into(), json!(wallet));
        }
        return Ok(());
    }

    if Confirm::new("Connect to Polymarket now and check your pUSD balance?").with_default(true).prompt()? {
        check_wallet(current, answers).await;
    }
    Ok(())
}

fn signer(key: &str) -> PrivateKeySigner {
    PrivateKeySigner::from_str(key).expect("validated by the prompt")
}

fn ask_private_key(current: &Settings) -> Result<String> {
    let mut prompt = Password::new("Private key (0x + 64 hex characters):")
        .with_display_mode(PasswordDisplayMode::Masked)
        .without_confirmation()
        .with_validator(|key: &str| {
            let key = key.trim();
            Ok(if key.is_empty() || PrivateKeySigner::from_str(key).is_ok() {
                Validation::Valid
            } else if key.trim_start_matches("0x").len() == 40 {
                Validation::Invalid("that is an address; the private key is 64 hex characters".into())
            } else {
                Validation::Invalid("not a valid private key (64 hex characters, optionally 0x-prefixed)".into())
            })
        });
    let mut help = "Stays on this machine; only used to sign orders".to_owned();
    if !current.polymarket_private_key.is_empty() {
        help += &format!(". Enter keeps the current key ({})", masked(&current.polymarket_private_key));
    }
    prompt = prompt.with_help_message(&help);
    loop {
        let key = prompt.clone().prompt()?.trim().to_owned();
        match (key.is_empty(), current.polymarket_private_key.is_empty()) {
            (false, _) => return Ok(key),
            (true, false) => return Ok(current.polymarket_private_key.clone()),
            (true, true) => println!("{}", style("A private key is needed to trade.").red()),
        }
    }
}

fn ask_wallet(current: &Settings, required: bool) -> Result<Option<Address>> {
    let validator = move |s: &str| {
        Ok(match s.trim() {
            "" if !required => Validation::Valid,
            s if Address::from_str(s).is_ok() => Validation::Valid,
            _ => Validation::Invalid("not an address (0x + 40 hex characters)".into()),
        })
    };
    let default = current.polymarket_deposit_wallet.map(|w| w.to_string()).unwrap_or_default();
    let label = if required { "Polymarket wallet address:" } else { "Wallet address (optional, Enter to skip):" };
    let text = Text::new(label).with_initial_value(&default).with_validator(validator).prompt()?;
    Ok(Address::from_str(text.trim()).ok())
}

async fn check_wallet(current: &Settings, answers: &Answers) {
    let mut doc = serde_json::to_value(current).expect("settings serialize");
    for (k, v) in &answers.changes {
        doc[k] = v.clone();
    }
    doc["polymarket_private_key"] = json!(answers.private_key);
    let settings: Settings = match serde_json::from_value(doc) {
        Ok(s) => s,
        Err(e) => return warn(&format!("could not check the wallet: {e}")),
    };
    let store = match Store::open(std::path::Path::new(":memory:")) {
        Ok(s) => s,
        Err(e) => return warn(&format!("could not check the wallet: {e}")),
    };
    match Executor::create(&settings, &store, true).await {
        Ok(ex) => {
            let balance = ex.collateral_balance_usd().await;
            ok(&format!("connected as {} ({})", ex.wallet_label(), ex.wallet_type()));
            match balance {
                Some(b) if b > 0.0 => ok(&format!("pUSD balance: ${b:.2}")),
                Some(_) => warn("pUSD balance is $0.00: deposit before trading live"),
                None => warn("could not read the pUSD balance"),
            }
        }
        Err(e) => warn(&format!("could not connect: {e:#}")),
    }
}

// --- 4. Risk limits ------------------------------------------------------------------------

fn limits_step(current: &Settings, answers: &mut Answers) -> Result<()> {
    section(4, "Risk limits");
    println!("Every order must fit under these caps. Start small; raise them once `jevmarket stats` looks good.");
    let per_trade = CustomType::<f64>::new("Max $ per trade:")
        .with_default(current.max_usd_per_trade)
        .with_validator(|v: &f64| Ok(positive(*v)))
        .with_help_message("Kelly sizing never goes above this")
        .prompt()?;
    let exposure = CustomType::<f64>::new("Max $ total exposure (positions + open orders):")
        .with_default(current.max_open_exposure_usd.max(per_trade))
        .with_validator(move |v: &f64| {
            Ok(if *v >= per_trade {
                Validation::Valid
            } else {
                Validation::Invalid("must be at least the per-trade cap".into())
            })
        })
        .with_help_message("Also the bankroll the Kelly fraction is applied to")
        .prompt()?;
    let trades = CustomType::<u32>::new("Max orders per run:")
        .with_default(current.max_trades_per_run)
        .with_validator(|v: &u32| Ok(positive(f64::from(*v))))
        .prompt()?;
    let dry_run =
        Confirm::new("Start in dry-run mode? `run` logs trades without placing orders until you switch it off")
            // Always offered as the default: pressing Enter must never switch a bot to live trading.
            .with_default(true)
            .prompt()?;

    for (key, value) in [
        ("max_usd_per_trade", json!(per_trade)),
        ("max_open_exposure_usd", json!(exposure)),
        ("max_trades_per_run", json!(trades)),
        ("dry_run", json!(dry_run)),
    ] {
        answers.changes.insert(key.into(), value);
    }
    Ok(())
}

fn positive(v: f64) -> Validation {
    if v > 0.0 { Validation::Valid } else { Validation::Invalid("must be greater than zero".into()) }
}

// --- save ----------------------------------------------------------------------------------

fn save_step(paths: &Paths, mut answers: Answers) -> Result<()> {
    section(5, "Save");
    let key_changed = answers.changes.contains_key("openrouter_api_key");
    let wants_secrets = key_changed || answers.private_key.is_some();
    let in_env = wants_secrets
        && Select::new(
            "Where should the keys live?",
            vec!["In the config file, readable only by you (recommended)", "In environment variables I export myself"],
        )
        .raw_prompt()?
        .index
            == 1;
    if in_env {
        // Drop keys an earlier setup stored, so the file and the environment cannot disagree.
        answers.changes.insert("openrouter_api_key".into(), Value::Null);
        answers.changes.insert("polymarket_private_key".into(), Value::Null);
    } else if let Some(key) = &answers.private_key {
        answers.changes.insert("polymarket_private_key".into(), json!(key));
    }

    println!("\nAbout to write to {}:", paths.config.display());
    for (key, value) in &answers.changes {
        let shown = match (key.as_str(), value) {
            ("openrouter_api_key" | "polymarket_private_key", Value::String(s)) => masked(s),
            ("openrouter_api_key" | "polymarket_private_key", Value::Null) => "removed (from environment)".into(),
            (_, Value::Null) => "(default)".into(),
            (_, v) => v.to_string(),
        };
        println!("  {key:<24} {shown}");
    }
    if !Confirm::new("Save?").with_default(true).prompt()? {
        bail!(InquireError::OperationCanceled);
    }
    config::update(&paths.config, answers.changes)?;
    ok(&format!("saved {}", paths.config.display()));

    if in_env {
        println!("\nAdd these to your shell profile (with your real values):");
        println!("  export OPENROUTER_API_KEY=sk-or-...");
        if answers.private_key.is_some() {
            println!("  export POLYMARKET_PRIVATE_KEY=0x...");
        }
    }

    println!("\n{}", style("Next steps").bold());
    let mut next = vec![
        ("jevmarket scan", "markets the bot would look at right now"),
        ("jevmarket decide <slug|url>", "research + Jev on one market, never trades"),
        ("jevmarket run --dry-run", "the full pipeline, orders only logged"),
    ];
    if answers.private_key.is_some() {
        next.push(("jevmarket setup", "create and approve your deposit wallet, then fund it"));
    }
    next.push(("jevmarket config set dry_run false", "when `stats` convinces you, go live"));
    for (i, (cmd, what)) in next.iter().enumerate() {
        println!("  {}. {}  {}", i + 1, style(cmd).cyan(), style(what).dim());
    }
    Ok(())
}

// --- output helpers ------------------------------------------------------------------------

fn section(n: u8, title: &str) {
    println!();
    ui::rule(&format!("{n}. {title}"));
}

fn step(n: u8, what: &str, url: &str) {
    println!("  {n}. {what}: {}", style(url).cyan().underlined());
}

fn step_text(n: u8, what: &str) {
    println!("  {n}. {what}");
}

fn ok(msg: &str) {
    println!("  {} {msg}", style("✓").green());
}

fn warn(msg: &str) {
    println!("  {} {msg}", style("!").yellow().bold());
}

fn masked(secret: &str) -> String {
    if secret.is_empty() {
        return "not set".into();
    }
    format!("…{}", secret.get(secret.len().saturating_sub(4)..).unwrap_or_default())
}

fn set_if_changed(answers: &mut Answers, key: &str, current: &str, chosen: &str) {
    if chosen != current {
        answers.changes.insert(key.into(), json!(chosen));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_keeps_text_models_with_prices_per_million() {
        let models = json!({"data": [
            {"id": "b/chat", "pricing": {"prompt": "0.000002", "completion": "0.00001"},
             "architecture": {"output_modalities": ["text"]}},
            {"id": "a/image", "pricing": {"prompt": "0", "completion": "0"},
             "architecture": {"output_modalities": ["image"]}},
            {"id": "a/chat", "pricing": {}}
        ]});
        let c = catalog(&models);
        assert_eq!(c.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["a/chat", "b/chat"]);
        assert_eq!(c[1].price(), "$2.00 in / $10.00 out per 1M tokens");
        assert_eq!(c[0].price(), "price unknown");
    }

    #[test]
    fn masks_secrets() {
        assert_eq!(masked(""), "not set");
        assert_eq!(masked("sk-or-v1-abcdef"), "…cdef");
    }
}
