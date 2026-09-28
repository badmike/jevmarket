//! Runtime settings, stored as JSON in the OS config directory.
//!
//! Location: `$XDG_CONFIG_HOME/jevmarket/config.json` (`~/.config/jevmarket/config.json`) on
//! Linux and macOS, `%APPDATA%\coderscantina\jevmarket\config\config.json` on Windows.
//! Secrets can be supplied through the environment instead of the file.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use etcetera::{AppStrategy as _, AppStrategyArgs, choose_app_strategy};
use polymarket_client_sdk_v2::types::Address;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::research::DEFAULT_EXCLUDE_DOMAINS;

/// Environment variables that override a config key. Secrets belong here or in a 0600 file.
pub const ENV_OVERRIDES: [(&str, &str); 8] = [
    ("OPENROUTER_API_KEY", "openrouter_api_key"),
    ("POLYMARKET_PRIVATE_KEY", "polymarket_private_key"),
    ("POLYMARKET_DEPOSIT_WALLET", "polymarket_deposit_wallet"),
    // The name before proxy and deposit wallets were configured apart.
    ("POLYMARKET_WALLET", "polymarket_deposit_wallet"),
    ("POLYMARKET_PROXY_WALLET", "polymarket_proxy_wallet"),
    ("POLYMARKET_BUILDER_API_KEY", "polymarket_builder_api_key"),
    ("POLYMARKET_BUILDER_SECRET", "polymarket_builder_secret"),
    ("POLYMARKET_BUILDER_PASSPHRASE", "polymarket_builder_passphrase"),
];

/// Keys that never leave the process: masked in `config show`, write-only in the console.
pub const SECRET_KEYS: [&str; 5] = [
    "openrouter_api_key",
    "polymarket_private_key",
    "polymarket_builder_api_key",
    "polymarket_builder_secret",
    "polymarket_builder_passphrase",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    // --- keys ---------------------------------------------------------------
    pub openrouter_api_key: String,
    pub polymarket_private_key: String,
    /// The deposit wallet orders are placed from: Polymarket only takes API orders from one.
    /// `jevmarket setup` creates it and sets this. Empty trades as a raw EOA.
    pub polymarket_deposit_wallet: Option<Address>,
    /// Your polymarket.com wallet (the proxy wallet of the key), the other end of `transfer`.
    /// Empty uses the proxy wallet derived from the key.
    pub polymarket_proxy_wallet: Option<Address>,
    /// Builder API credentials (polymarket.com, Settings, Builder). Only `setup` needs them, to
    /// deploy the deposit wallet and approve the exchange through Polymarket's gas-free relayer.
    pub polymarket_builder_api_key: String,
    pub polymarket_builder_secret: String,
    pub polymarket_builder_passphrase: String,

    // --- endpoints ----------------------------------------------------------
    pub openrouter_base_url: String,
    pub clob_host: String,
    pub polygon_rpc_url: String,

    // --- Jev ----------------------------------------------------------------
    pub jev_model: String,
    /// Put the market midpoint into the Jev state as `market_implied_probability_yes`.
    /// `stats` compares Brier scores of both variants.
    pub jev_sees_market_price: bool,

    // --- researcher (generative model + web search via OpenRouter) ---------
    pub research_enabled: bool,
    pub research_model: String,
    /// Reuse a cached brief for this long.
    pub research_ttl_hours: f64,
    /// Hard cap on researcher calls per `run` pass. In the daemon, per research cycle, shared
    /// with the price watch until the next cycle starts.
    pub max_research_per_run: u32,
    /// Web search results per brief.
    pub research_max_results: u32,
    /// Evidence block size in the Jev state.
    pub research_max_chars: usize,
    /// Odds sites and mirrors the web search must never return.
    pub research_exclude_domains: Vec<String>,
    /// Markets researched and priced in parallel during `run`.
    pub concurrency: usize,
    /// A cached brief is researched again once the midpoint moved more than this since it was written.
    pub research_max_price_move: f64,

    // --- signal thresholds --------------------------------------------------
    /// `P_jev - best ask` required to trade.
    pub min_edge: f64,
    /// Jev's belief that the state holds enough information to judge the question.
    pub min_answerable: f64,
    /// 0..4 score of how unambiguous the resolution criteria are.
    pub min_clarity: u8,
    /// Only buy contracts priced inside this band. Jev tends to be under-confident at the
    /// extremes, so "edge" on 5-cent longshots is usually the model being wrong, not the market.
    pub min_trade_price: f64,
    pub max_trade_price: f64,
    /// Edges above this are more likely a model error than a mispricing: logged, and skipped
    /// unless a fresh brief brings them back below.
    pub suspicious_edge: f64,

    // --- market filter ------------------------------------------------------
    pub min_liquidity_usd: f64,
    pub min_volume_usd: f64,
    pub max_days_to_resolution: i64,
    pub max_spread: f64,
    /// Skip markets already priced at the extremes: no room for edge, wasted Jev calls.
    pub min_market_price: f64,
    pub max_market_price: f64,
    pub description_max_chars: usize,
    /// Skip markets with any of these Polymarket tags. The defaults are settled by a live price,
    /// a post count or a single game: research cannot make Jev answer them, so they only burn
    /// briefs and candidate slots.
    pub exclude_tags: Vec<String>,

    // --- hard caps (enforced in the executor) -------------------------------
    pub max_usd_per_trade: f64,
    pub max_open_exposure_usd: f64,
    /// Orders per `run` pass. In the daemon, per research cycle, shared with the price watch.
    pub max_trades_per_run: u32,
    pub kelly_fraction: f64,

    // --- price watch (daemon only) ------------------------------------------
    /// Seconds between price-watch ticks between research cycles; 0 turns the watch off.
    pub watch_interval_secs: u64,
    /// A watch signal on a brief older than this is researched again before trading.
    pub trade_brief_max_age_minutes: u64,
    /// Let the price watch sell held positions early, see `min_exit_edge` and `min_exit_profit`.
    pub sell_early: bool,
    /// Sell only when the best bid beats Jev's probability of the held side by this much.
    pub min_exit_edge: f64,
    /// And only when the bid returns at least this share of what the position cost (0.2 = 20%).
    pub min_exit_profit: f64,

    // --- misc ---------------------------------------------------------------
    pub dry_run: bool,
    /// SQLite log; defaults to the OS data directory.
    pub db_path: Option<PathBuf>,
    /// `tracing` filter for the `jevmarket` target; `RUST_LOG` wins when set.
    pub log_level: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            openrouter_api_key: String::new(),
            polymarket_private_key: String::new(),
            polymarket_deposit_wallet: None,
            polymarket_proxy_wallet: None,
            polymarket_builder_api_key: String::new(),
            polymarket_builder_secret: String::new(),
            polymarket_builder_passphrase: String::new(),
            openrouter_base_url: "https://openrouter.ai/api".into(),
            clob_host: "https://clob.polymarket.com".into(),
            polygon_rpc_url: "https://polygon-bor-rpc.publicnode.com".into(),
            jev_model: "typesafe/jev-1.13".into(),
            jev_sees_market_price: true,
            research_enabled: true,
            research_model: "deepseek/deepseek-v4-pro-0813".into(),
            research_ttl_hours: 6.0,
            max_research_per_run: 20,
            research_max_results: 5,
            research_max_chars: 2500,
            research_exclude_domains: DEFAULT_EXCLUDE_DOMAINS.map(String::from).to_vec(),
            concurrency: 4,
            research_max_price_move: 0.05,
            min_edge: 0.08,
            min_answerable: 0.70,
            min_clarity: 2,
            min_trade_price: 0.10,
            max_trade_price: 0.90,
            suspicious_edge: 0.25,
            min_liquidity_usd: 5_000.0,
            min_volume_usd: 10_000.0,
            max_days_to_resolution: 60,
            max_spread: 0.06,
            min_market_price: 0.03,
            max_market_price: 0.97,
            description_max_chars: 1_500,
            exclude_tags: ["Crypto Prices", "Hit Price", "Tweet Markets", "Games"].map(String::from).to_vec(),
            max_usd_per_trade: 5.0,
            max_open_exposure_usd: 50.0,
            max_trades_per_run: 3,
            kelly_fraction: 0.25,
            watch_interval_secs: 60,
            trade_brief_max_age_minutes: 120,
            sell_early: true,
            min_exit_edge: 0.05,
            min_exit_profit: 0.20,
            dry_run: false,
            db_path: None,
            log_level: "info".into(),
        }
    }
}

/// Where jevmarket keeps its files.
#[derive(Debug, Clone)]
pub struct Paths {
    pub config: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    /// OS defaults, or `config_override` for the config file.
    pub fn resolve(config_override: Option<PathBuf>) -> Result<Self> {
        let strategy = choose_app_strategy(AppStrategyArgs {
            top_level_domain: "com".into(),
            author: "coderscantina".into(),
            app_name: "jevmarket".into(),
        })
        .context("could not determine the home directory")?;
        Ok(Self {
            config: config_override.unwrap_or_else(|| strategy.config_dir().join("config.json")),
            data_dir: strategy.data_dir(),
        })
    }
}

impl Settings {
    /// Load the config file (missing file = defaults) and apply environment overrides.
    pub fn load(path: &Path) -> Result<Self> {
        let mut doc = read_doc(path)?;
        for (var, key) in ENV_OVERRIDES {
            if let Ok(v) = std::env::var(var)
                && !v.trim().is_empty()
            {
                doc.insert(key.into(), Value::String(v.trim().into()));
            }
        }
        serde_json::from_value(Value::Object(doc)).with_context(|| format!("invalid config {}", path.display()))
    }

    pub fn db_path(&self, paths: &Paths) -> PathBuf {
        self.db_path.clone().unwrap_or_else(|| paths.data_dir.join("jevmarket.db"))
    }

    pub fn require_openrouter_key(&self) -> Result<&str> {
        if self.openrouter_api_key.is_empty() {
            bail!(
                "no OpenRouter key: run `jevmarket init`, `jevmarket config set openrouter_api_key sk-or-...` \
                 or export OPENROUTER_API_KEY"
            );
        }
        Ok(&self.openrouter_api_key)
    }

    /// Effective settings as JSON with secrets masked, for `config show`.
    pub fn redacted(&self) -> Value {
        let mut v = serde_json::to_value(self).expect("settings serialize");
        for key in SECRET_KEYS {
            if let Some(Value::String(s)) = v.get_mut(key)
                && !s.is_empty()
            {
                let tail = s.get(s.len().saturating_sub(4)..).unwrap_or_default();
                *s = format!("…{tail}");
            }
        }
        v
    }
}

/// Write a full config file with every default, so all knobs are discoverable.
pub fn init(path: &Path, force: bool) -> Result<()> {
    if path.exists() && !force {
        bail!("{} already exists (use --force to overwrite)", path.display());
    }
    let Value::Object(doc) = serde_json::to_value(Settings::default())? else {
        unreachable!("settings serialize to an object")
    };
    write_doc(path, &doc)
}

/// Set one key in the config file. `raw` is parsed as JSON when possible, else taken as a string.
/// Only the touched key is written, so untouched keys keep following the built-in defaults.
pub fn set(path: &Path, key: &str, raw: &str) -> Result<()> {
    let value = serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.into()));
    update(path, Map::from_iter([(key.to_owned(), value)])).with_context(|| format!("rejected `{key}`"))
}

/// Merge `changes` into the config file after validating the result. `null` values are dropped,
/// so those keys fall back to their defaults.
pub fn update(path: &Path, changes: Map<String, Value>) -> Result<()> {
    let mut doc = read_doc(path)?;
    for (key, value) in changes {
        if value.is_null() {
            doc.remove(&key);
        } else {
            doc.insert(key, value);
        }
    }
    serde_json::from_value::<Settings>(Value::Object(doc.clone()))?;
    write_doc(path, &doc)
}

/// Keys renamed since they shipped: `(old, new)`. Read under the new name, written back under it.
const RENAMED_KEYS: [(&str, &str); 1] = [("polymarket_wallet", "polymarket_deposit_wallet")];

fn read_doc(path: &Path) -> Result<Map<String, Value>> {
    let mut doc: Map<String, Value> = match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).with_context(|| format!("{} is not a JSON object", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    for (old, new) in RENAMED_KEYS {
        if let Some(value) = doc.remove(old) {
            doc.entry(new).or_insert(value);
        }
    }
    Ok(doc)
}

fn write_doc(path: &Path, doc: &Map<String, Value>) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let mut text = serde_json::to_string_pretty(doc)?;
    text.push('\n');
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    // The file may hold a private key: owner read/write only, from the first byte on.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let write = |mut file: fs::File| std::io::Write::write_all(&mut file, text.as_bytes());
    options.open(path).and_then(write).with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        // `mode` only applies to new files; tighten files created by an older version or by hand.
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).context("chmod 600 config")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_validates_and_keeps_file_minimal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        set(&path, "min_edge", "0.1").unwrap();
        set(&path, "jev_model", "typesafe/jev-latest").unwrap();
        assert!(set(&path, "min_egde", "0.1").is_err(), "unknown keys are rejected");
        assert!(set(&path, "max_trades_per_run", "lots").is_err(), "wrong types are rejected");
        set(&path, "jev_model", "null").unwrap();
        set(&path, "jev_model", "typesafe/jev-latest").unwrap();

        let doc = read_doc(&path).unwrap();
        assert_eq!(doc.len(), 2);
        let s = Settings::load(&path).unwrap();
        assert_eq!(s.min_edge, 0.1);
        assert_eq!(s.jev_model, "typesafe/jev-latest");
        assert_eq!(s.max_trades_per_run, 3);
    }

    #[test]
    fn renamed_keys_migrate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, r#"{"polymarket_wallet": "0x0b89b53e12F1470D6884D88B511Ae3801986DB3D"}"#).unwrap();
        assert!(Settings::load(&path).unwrap().polymarket_deposit_wallet.is_some());
        set(&path, "min_edge", "0.1").unwrap();
        let doc = read_doc(&path).unwrap();
        assert!(
            doc.contains_key("polymarket_deposit_wallet")
                && !fs::read_to_string(&path).unwrap().contains("\"polymarket_wallet\"")
        );
    }

    #[test]
    fn redacted_masks_secrets() {
        let s = Settings { openrouter_api_key: "sk-or-v1-abcdef".into(), ..Settings::default() };
        let v = s.redacted();
        assert_eq!(v["openrouter_api_key"], "…cdef");
        assert_eq!(v["polymarket_private_key"], "");
        let s = Settings { polymarket_builder_passphrase: "passphrase".into(), ..Settings::default() };
        assert_eq!(s.redacted()["polymarket_builder_passphrase"], "…rase");
    }
}
