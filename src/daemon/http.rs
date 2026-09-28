//! The HTTP API, the event stream and the embedded console.
//!
//! There is no login, so the rules that stand in for one live here: secrets never leave the
//! process, anything that can put money at risk needs a typed confirmation, state-changing
//! requests must come from the console's own origin, and a `Host` that is neither local nor
//! forwarded by a proxy is refused (DNS rebinding).

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::extract::{Path, Query, Request, State};
use axum::http::header::{HOST, ORIGIN};
use axum::http::{HeaderMap, HeaderName, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::sse::{self, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, oneshot, watch};

use super::api::{
    BriefRecord, BriefSummary, ConfigView, Event, OrderEvent, Positions, Recommendation, RecommendationDetail,
    SecretState, StatsView, Status,
};
use super::db::Db;
use super::engine::Command;
use super::hub::Hub;
use super::{Options, assets, settings};
use crate::config::{self, ENV_OVERRIDES, Paths, Settings};

/// What the console must send in `confirm` before anything can place real orders.
pub const LIVE_CONFIRM: &str = "LIVE";

pub struct AppState {
    pub paths: Paths,
    pub opts: Options,
    pub hub: Arc<Hub>,
    pub db: Mutex<Db>,
    pub commands: mpsc::Sender<Command>,
    pub shutdown: watch::Receiver<bool>,
    pub index_html: Option<String>,
}

type Shared = State<Arc<AppState>>;

impl AppState {
    fn db(&self) -> MutexGuard<'_, Db> {
        self.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn settings(&self) -> Result<Settings, ApiError> {
        settings(&self.paths, self.opts.dry_run_forced).map_err(ApiError::internal)
    }

    /// Send a command to the engine and wait for its answer.
    async fn ask<T>(&self, command: impl FnOnce(oneshot::Sender<anyhow::Result<T>>) -> Command) -> Result<T, ApiError> {
        let (reply, answer) = oneshot::channel();
        self.commands.send(command(reply)).await.map_err(|_| ApiError::unavailable())?;
        answer.await.map_err(|_| ApiError::unavailable())?.map_err(ApiError::failed)
    }

    async fn tell(&self, command: Command) -> Result<(), ApiError> {
        self.commands.send(command).await.map_err(|_| ApiError::unavailable())
    }
}

pub fn router(state: AppState, base_path: &str) -> Router {
    let api = Router::new()
        .route("/status", get(status))
        .route("/events", get(events))
        .route("/activity", get(activity))
        .route("/config", get(read_config).patch(patch_config))
        .route("/recommendations", get(recommendations))
        .route("/recommendations/{slug}", get(recommendation))
        .route("/briefs", get(briefs))
        .route("/briefs/{id}", get(brief))
        .route("/briefs/refresh", post(refresh_brief))
        .route("/orders", get(orders))
        .route("/positions", get(positions))
        .route("/stats", get(stats))
        .route("/pass", post(run_pass))
        .route("/loop/pause", post(pause))
        .route("/loop/resume", post(resume))
        .route("/decide", post(decide))
        .fallback(|| async { ApiError::new(StatusCode::NOT_FOUND, "no such endpoint") });
    let app = Router::new()
        .nest("/api", api)
        .fallback(|State(st): Shared, uri: Uri| async move { assets::serve(uri.path(), st.index_html.as_deref()) })
        .layer(middleware::from_fn(guard))
        .with_state(Arc::new(state));
    if base_path.is_empty() { app } else { Router::new().nest(base_path, app) }
}

// --- safety -----------------------------------------------------------------------------------

async fn guard(req: Request, next: Next) -> Response {
    if let Err(why) = check_request(req.method(), req.headers()) {
        tracing::warn!(method = %req.method(), uri = %req.uri(), "refused: {why}");
        return ApiError::new(StatusCode::FORBIDDEN, why).into_response();
    }
    tracing::debug!(method = %req.method(), uri = %req.uri(), client = header(req.headers(), "x-forwarded-for"));
    next.run(req).await
}

/// Refuse DNS rebinding and cross-origin writes. There are no cookies, but a page on another
/// site can still make the browser send requests to a daemon on localhost.
pub fn check_request(method: &Method, h: &HeaderMap) -> Result<(), String> {
    let forwarded_host = header(h, "x-forwarded-host");
    let host = forwarded_host.or_else(|| header(h, HOST.as_str())).ok_or("missing Host header")?;
    if forwarded_host.is_none() && !is_local(host) {
        return Err(format!(
            "unexpected Host {host}: open the console by IP address or localhost, or through a proxy that sets \
             X-Forwarded-Host"
        ));
    }
    if method.is_safe() {
        return Ok(());
    }
    if let Some(site) = header(h, "sec-fetch-site").filter(|site| *site != "same-origin") {
        return Err(format!("cross-site request refused ({site})"));
    }
    if let Some(origin) = header(h, ORIGIN.as_str()) {
        let proto = header(h, "x-forwarded-proto").unwrap_or("http");
        if origin != format!("{proto}://{host}") {
            return Err(format!("cross-origin request from {origin} refused"));
        }
    }
    Ok(())
}

/// First value of a header, trimmed; proxies append to `X-Forwarded-*` lists.
fn header<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let value = h.get(name)?.to_str().ok()?;
    Some(value.split(',').next().unwrap_or(value).trim()).filter(|v| !v.is_empty())
}

/// `localhost` or an IP literal, with or without a port. A DNS name could be rebound to us.
fn is_local(host: &str) -> bool {
    let name = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => host.rsplit_once(':').filter(|(_, port)| port.parse::<u16>().is_ok()).map_or(host, |(name, _)| name),
    };
    name.eq_ignore_ascii_case("localhost") || name.parse::<std::net::IpAddr>().is_ok()
}

/// Money is at risk unless the effective setting is a dry run: demand the typed confirmation.
pub fn require_confirm(live: bool, confirm: Option<&str>) -> Result<(), ApiError> {
    if live && confirm != Some(LIVE_CONFIRM) {
        return Err(ApiError {
            status: StatusCode::PRECONDITION_REQUIRED,
            body: ErrorBody {
                error: format!("this places real orders: repeat the request with \"confirm\": \"{LIVE_CONFIRM}\""),
                confirm: Some(LIVE_CONFIRM),
                fields: BTreeMap::new(),
            },
        });
    }
    Ok(())
}

/// Anything named `*_key` is a secret: write-only, reported as set or not.
fn is_secret(key: &str) -> bool {
    key.ends_with("_key")
}

// --- errors -----------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    error: String,
    /// The confirmation phrase the request is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    confirm: Option<&'static str>,
    /// Per-setting validation errors.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    fields: BTreeMap<String, String>,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    body: ErrorBody,
}

impl ApiError {
    fn new(status: StatusCode, error: impl Into<String>) -> Self {
        Self { status, body: ErrorBody { error: error.into(), confirm: None, fields: BTreeMap::new() } }
    }

    fn internal(e: anyhow::Error) -> Self {
        tracing::error!("{e:#}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }

    /// An action the engine tried and could not complete.
    fn failed(e: anyhow::Error) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, format!("{e:#}"))
    }

    fn unavailable() -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "the engine is shutting down")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

// --- queries ----------------------------------------------------------------------------------

async fn status(State(st): Shared) -> Json<Status> {
    Json(st.hub.status())
}

async fn activity(State(st): Shared) -> Json<Vec<Event>> {
    Json(st.hub.recent())
}

#[derive(Deserialize)]
struct Limit {
    limit: Option<u32>,
}

impl Limit {
    fn or(&self, default: u32) -> u32 {
        self.limit.unwrap_or(default).clamp(1, 1000)
    }
}

async fn recommendations(State(st): Shared, Query(q): Query<Limit>) -> ApiResult<Vec<Recommendation>> {
    Ok(Json(st.db().recommendations(q.or(300)).map_err(ApiError::internal)?))
}

async fn recommendation(State(st): Shared, Path(slug): Path<String>) -> ApiResult<RecommendationDetail> {
    let ttl = st.settings()?.research_ttl_hours * 3600.0;
    let detail = st.db().recommendation_detail(&slug, ttl).map_err(ApiError::internal)?;
    detail.map(Json).ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, format!("no decision for {slug}")))
}

async fn briefs(State(st): Shared, Query(q): Query<Limit>) -> ApiResult<Vec<BriefSummary>> {
    let ttl = st.settings()?.research_ttl_hours * 3600.0;
    Ok(Json(st.db().briefs(q.or(300), ttl).map_err(ApiError::internal)?))
}

async fn brief(State(st): Shared, Path(id): Path<i64>) -> ApiResult<BriefRecord> {
    let ttl = st.settings()?.research_ttl_hours * 3600.0;
    let brief = st.db().brief(id, ttl).map_err(ApiError::internal)?;
    brief.map(Json).ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, format!("no brief {id}")))
}

async fn orders(State(st): Shared, Query(q): Query<Limit>) -> ApiResult<Vec<OrderEvent>> {
    Ok(Json(st.db().orders(q.or(100)).map_err(ApiError::internal)?))
}

async fn stats(State(st): Shared) -> ApiResult<StatsView> {
    Ok(Json(st.db().stats().map_err(ApiError::internal)?))
}

async fn positions(State(st): Shared) -> ApiResult<Positions> {
    st.ask(Command::Positions).await.map(Json)
}

/// Server-sent events: every [`Event`] as JSON in `data`, a comment every 15 s so proxies keep
/// the connection, and the end of the stream on shutdown so the server can stop.
async fn events(State(st): Shared) -> impl IntoResponse {
    let mut shutdown = st.shutdown.clone();
    let stream = futures::stream::unfold(st.hub.subscribe(), |mut rx| async move {
        let event = match rx.recv().await {
            Ok(event) => event,
            Err(RecvError::Lagged(_)) => Event::Resync,
            Err(RecvError::Closed) => return None,
        };
        Some((Ok::<_, Infallible>(sse_event(&event)), rx))
    })
    .take_until(async move {
        let _ = shutdown.wait_for(|stop| *stop).await;
    });
    let sse = Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)));
    // Nginx buffers responses by default, which would hold events back.
    ([(HeaderName::from_static("x-accel-buffering"), "no")], sse)
}

pub fn sse_event(event: &Event) -> sse::Event {
    sse::Event::default().data(serde_json::to_string(event).unwrap_or_default())
}

// --- config -----------------------------------------------------------------------------------

async fn read_config(State(st): Shared) -> ApiResult<ConfigView> {
    config_view(&st.paths, st.opts.dry_run_forced).map(Json).map_err(ApiError::internal)
}

/// Effective settings without secret values.
pub fn config_view(paths: &Paths, dry_run_forced: bool) -> anyhow::Result<ConfigView> {
    let s = Settings::load(&paths.config)?;
    let (Value::Object(mut values), Value::Object(mut defaults)) =
        (serde_json::to_value(&s)?, serde_json::to_value(Settings::default())?)
    else {
        unreachable!("settings serialize to an object")
    };
    let secret_keys: Vec<String> = values.keys().filter(|k| is_secret(k)).cloned().collect();
    let secrets = secret_keys
        .iter()
        .map(|key| {
            defaults.remove(key);
            let set = values.remove(key).is_some_and(|v| v.as_str().is_some_and(|s| !s.is_empty()));
            SecretState { key: key.clone(), set }
        })
        .collect();
    let env_overrides = ENV_OVERRIDES
        .iter()
        .filter(|(var, _)| std::env::var(var).is_ok_and(|v| !v.trim().is_empty()))
        .map(|(_, key)| (*key).to_owned())
        .collect();
    Ok(ConfigView {
        path: paths.config.display().to_string(),
        values,
        defaults,
        secrets,
        env_overrides,
        dry_run_forced,
    })
}

#[derive(Deserialize)]
struct ConfigPatch {
    /// Keys to set; `null` returns a key to its default.
    changes: Map<String, Value>,
    confirm: Option<String>,
}

async fn patch_config(State(st): Shared, Json(patch): Json<ConfigPatch>) -> ApiResult<ConfigView> {
    apply_config(&st.paths, patch.changes, patch.confirm.as_deref())?;
    let dry_run = st.settings()?.dry_run;
    st.hub.update(|s| s.dry_run = dry_run);
    st.hub.emit(Event::ConfigChanged);
    tracing::info!("config updated");
    read_config(State(st)).await
}

/// Validate each change on its own for inline errors, require the confirmation for switching
/// dry runs off, then write through `config::update`, the path `config set` takes.
pub fn apply_config(paths: &Paths, changes: Map<String, Value>, confirm: Option<&str>) -> Result<(), ApiError> {
    let fields = field_errors(&changes);
    if !fields.is_empty() {
        let mut e = ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "some settings are invalid");
        e.body.fields = fields;
        return Err(e);
    }
    let current = Settings::load(&paths.config).map_err(ApiError::internal)?;
    let mut next = serde_json::to_value(&current).map_err(|e| ApiError::internal(e.into()))?;
    let defaults = serde_json::to_value(Settings::default()).map_err(|e| ApiError::internal(e.into()))?;
    for (key, value) in &changes {
        next[key] = if value.is_null() { defaults[key].clone() } else { value.clone() };
    }
    let next: Settings = serde_json::from_value(next).map_err(|e| ApiError::failed(e.into()))?;
    require_confirm(current.dry_run && !next.dry_run, confirm)?;
    config::update(&paths.config, changes).map_err(ApiError::failed)
}

/// Unknown keys and values of the wrong type, by key.
fn field_errors(changes: &Map<String, Value>) -> BTreeMap<String, String> {
    let Ok(Value::Object(known)) = serde_json::to_value(Settings::default()) else { return BTreeMap::new() };
    let mut errors = BTreeMap::new();
    for (key, value) in changes {
        if !known.contains_key(key) {
            errors.insert(key.clone(), "unknown setting".into());
        } else if !value.is_null()
            && let Err(e) =
                serde_json::from_value::<Settings>(Value::Object(Map::from_iter([(key.clone(), value.clone())])))
        {
            errors.insert(key.clone(), e.to_string());
        }
    }
    errors
}

// --- actions ----------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct Confirm {
    confirm: Option<String>,
}

/// Start a pass now. Taken as JSON, like every write: a cross-site form cannot send that
/// without a CORS preflight, which this server never answers.
async fn run_pass(State(st): Shared, Json(body): Json<Confirm>) -> Result<StatusCode, ApiError> {
    require_confirm(!st.settings()?.dry_run, body.confirm.as_deref())?;
    st.ask(Command::RunPass).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn pause(State(st): Shared, Json(_): Json<Confirm>) -> Result<StatusCode, ApiError> {
    st.tell(Command::Pause).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn resume(State(st): Shared, Json(body): Json<Confirm>) -> Result<StatusCode, ApiError> {
    require_confirm(!st.settings()?.dry_run, body.confirm.as_deref())?;
    st.tell(Command::Resume).await?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
struct MarketRef {
    /// Slug or polymarket.com URL.
    reference: String,
    #[serde(default)]
    fresh: bool,
}

impl MarketRef {
    fn reference(&self) -> Result<String, ApiError> {
        let r = self.reference.trim();
        if r.is_empty() {
            return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "give a market slug or polymarket.com URL"));
        }
        Ok(r.to_owned())
    }
}

/// Research and ask Jev about one market. Logs the decision, never trades.
async fn decide(State(st): Shared, Json(body): Json<MarketRef>) -> ApiResult<Recommendation> {
    let reference = body.reference()?;
    st.ask(|reply| Command::Decide { reference, fresh: body.fresh, reply }).await.map(Json)
}

async fn refresh_brief(State(st): Shared, Json(body): Json<MarketRef>) -> ApiResult<BriefRecord> {
    let reference = body.reference()?;
    st.ask(|reply| Command::RefreshBrief { reference, reply }).await.map(Json)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;
    use serde_json::json;

    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (name, value) in pairs {
            h.insert(*name, HeaderValue::from_str(value).unwrap());
        }
        h
    }

    #[test]
    fn same_origin_writes_pass() {
        let local = headers(&[
            ("host", "127.0.0.1:8787"),
            ("origin", "http://127.0.0.1:8787"),
            ("sec-fetch-site", "same-origin"),
        ]);
        assert!(check_request(&Method::POST, &local).is_ok());
        let proxied = headers(&[
            ("host", "127.0.0.1:8787"),
            ("x-forwarded-host", "bot.example.com"),
            ("x-forwarded-proto", "https"),
            ("origin", "https://bot.example.com"),
        ]);
        assert!(check_request(&Method::PATCH, &proxied).is_ok());
        assert!(check_request(&Method::POST, &headers(&[("host", "localhost:8787")])).is_ok(), "curl sends no Origin");
    }

    #[test]
    fn cross_origin_writes_are_refused() {
        let evil = headers(&[("host", "127.0.0.1:8787"), ("origin", "https://evil.example")]);
        assert!(check_request(&Method::POST, &evil).is_err());
        assert!(check_request(&Method::GET, &evil).is_ok(), "reads carry no risk and are not CORS-readable");
        let cross_site = headers(&[("host", "localhost:8787"), ("sec-fetch-site", "cross-site")]);
        assert!(check_request(&Method::DELETE, &cross_site).is_err());
        let wrong_scheme = headers(&[
            ("host", "127.0.0.1:8787"),
            ("x-forwarded-host", "bot.example.com"),
            ("x-forwarded-proto", "https"),
            ("origin", "http://bot.example.com"),
        ]);
        assert!(check_request(&Method::POST, &wrong_scheme).is_err());
    }

    #[test]
    fn rebound_host_names_are_refused() {
        assert!(check_request(&Method::GET, &headers(&[("host", "evil.example:8787")])).is_err());
        assert!(check_request(&Method::GET, &headers(&[("host", "[::1]:8787")])).is_ok());
        assert!(check_request(&Method::GET, &headers(&[("host", "192.168.1.20")])).is_ok());
        assert!(check_request(&Method::GET, &HeaderMap::new()).is_err());
    }

    #[test]
    fn live_actions_need_the_typed_confirmation() {
        assert!(require_confirm(false, None).is_ok());
        let e = require_confirm(true, Some("yes")).unwrap_err();
        assert_eq!((e.status, e.body.confirm), (StatusCode::PRECONDITION_REQUIRED, Some(LIVE_CONFIRM)));
        assert!(require_confirm(true, Some(LIVE_CONFIRM)).is_ok());
    }

    fn paths(dir: &tempfile::TempDir) -> Paths {
        Paths { config: dir.path().join("config.json"), data_dir: dir.path().to_owned() }
    }

    #[test]
    fn secrets_never_leave_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        let key = "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318";
        config::set(&p.config, "polymarket_private_key", key).unwrap();
        let view = serde_json::to_string(&config_view(&p, false).unwrap()).unwrap();
        assert!(!view.contains(key) && !view.contains(&key[key.len() - 4..]), "{view}");
        assert!(view.contains(r#"{"key":"polymarket_private_key","set":true}"#), "{view}");
        assert!(!view.contains("\"openrouter_api_key\":"), "no secret key among values or defaults");
    }

    #[test]
    fn switching_dry_run_off_needs_the_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        config::set(&p.config, "dry_run", "true").unwrap();
        let off = || Map::from_iter([("dry_run".to_owned(), json!(false))]);
        let e = apply_config(&p, off(), None).unwrap_err();
        assert_eq!(e.status, StatusCode::PRECONDITION_REQUIRED);
        let reset = Map::from_iter([("dry_run".to_owned(), Value::Null)]);
        assert_eq!(
            apply_config(&p, reset, None).unwrap_err().status,
            StatusCode::PRECONDITION_REQUIRED,
            "default is live"
        );
        assert!(Settings::load(&p.config).unwrap().dry_run, "nothing written");
        apply_config(&p, off(), Some(LIVE_CONFIRM)).unwrap();
        assert!(!Settings::load(&p.config).unwrap().dry_run);
    }

    #[test]
    fn config_errors_are_reported_per_setting() {
        let dir = tempfile::tempdir().unwrap();
        let changes = Map::from_iter([
            ("min_egde".to_owned(), json!(0.1)),
            ("max_trades_per_run".to_owned(), json!("lots")),
            ("min_edge".to_owned(), json!(0.1)),
        ]);
        let e = apply_config(&paths(&dir), changes, None).unwrap_err();
        assert_eq!(e.body.fields.keys().collect::<Vec<_>>(), ["max_trades_per_run", "min_egde"]);
        assert!(!dir.path().join("config.json").exists(), "nothing written");
    }
}
