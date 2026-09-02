//! Thin `#[tauri::command]` wrappers around `AppState`/`tollgate::store`.
//! No business logic lives here: every command either mutates config
//! through [`crate::state::AppState::update_config`] or runs an existing
//! `store::` query on a blocking thread, mirroring how `gateway::handle_stats`
//! already offloads SQLite work off the async runtime.

use chrono::{DateTime, TimeDelta, Utc};
use tollgate::config::{Config, ModelPrice, Route};
use tollgate::store::{self, Record, RequestFilter, RequestSummary, StatRow, TimeSeriesResult};
use serde::Serialize;
use tauri::State;

use crate::state::AppState;

fn to_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// These commands are only ever called by this app's own frontend (not a
/// stable external API), so `since`/`until` are plain RFC3339 absolute
/// timestamps computed client-side — by a preset ("now minus 24h") or a
/// custom range picker — rather than duration strings the backend would
/// have to re-anchor to its own "now".
fn parse_ts(raw: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(raw).map(|dt| dt.with_timezone(&Utc)).map_err(to_err)
}

fn parse_until(raw: &Option<String>) -> Result<Option<DateTime<Utc>>, String> {
    match raw {
        Some(s) if !s.is_empty() => Ok(Some(parse_ts(s)?)),
        _ => Ok(None),
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn get_config(state: State<AppState>) -> Config {
    state.config.lock().expect("config mutex poisoned").clone()
}

#[tauri::command(rename_all = "snake_case")]
pub fn save_route(state: State<AppState>, route: Route) -> Result<(), String> {
    state
        .update_config(|cfg| {
            if let Some(existing) = cfg.routes.iter_mut().find(|r| r.name == route.name) {
                *existing = route.clone();
            } else {
                cfg.routes.push(route.clone());
            }
        })
        .map_err(to_err)
}

#[tauri::command(rename_all = "snake_case")]
pub fn delete_route(state: State<AppState>, name: String) -> Result<(), String> {
    state.update_config(|cfg| cfg.routes.retain(|r| r.name != name)).map_err(to_err)
}

#[tauri::command(rename_all = "snake_case")]
pub fn save_pricing(state: State<AppState>, model: String, price: ModelPrice) -> Result<(), String> {
    state
        .update_config(|cfg| {
            cfg.pricing.insert(model.clone(), price);
        })
        .map_err(to_err)
}

#[tauri::command(rename_all = "snake_case")]
pub fn delete_pricing(state: State<AppState>, model: String) -> Result<(), String> {
    state
        .update_config(|cfg| {
            cfg.pricing.remove(&model);
        })
        .map_err(to_err)
}

#[tauri::command(rename_all = "snake_case")]
pub fn save_general_settings(state: State<AppState>, max_body_bytes: i64, shutdown_timeout: String) -> Result<(), String> {
    state
        .update_config(|cfg| {
            cfg.max_body_bytes = max_body_bytes;
            cfg.shutdown_timeout = shutdown_timeout.clone();
        })
        .map_err(to_err)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_stats(state: State<'_, AppState>, since: String, until: Option<String>, group_by: String) -> Result<Vec<StatRow>, String> {
    let conn = state.store.read_conn();
    tokio::task::spawn_blocking(move || {
        let since_dt = parse_ts(&since)?;
        let until_dt = parse_until(&until)?;
        let conn = conn.lock().expect("read connection mutex poisoned");
        store::stats(&conn, &group_by, since_dt, until_dt).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_time_series(
    state: State<'_, AppState>,
    since: String,
    until: Option<String>,
    group_by: String,
    metric: String,
    bucket: Option<String>,
) -> Result<TimeSeriesResult, String> {
    let conn = state.store.read_conn();
    tokio::task::spawn_blocking(move || {
        let since_dt = parse_ts(&since)?;
        let until_dt = parse_until(&until)?;
        let bucket_delta = match bucket {
            Some(b) if !b.is_empty() => {
                let d = humantime::parse_duration(&b).map_err(to_err)?;
                TimeDelta::from_std(d).map_err(to_err)?
            }
            _ => store::default_bucket(until_dt.unwrap_or_else(Utc::now) - since_dt),
        };
        let conn = conn.lock().expect("read connection mutex poisoned");
        store::time_series(&conn, &group_by, &metric, since_dt, until_dt, bucket_delta).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

/// Grouped into one struct (rather than seven separate command
/// arguments) purely to keep clippy's `too_many_arguments` happy; the
/// frontend invokes this as `list_requests({ args: {...} })`.
#[derive(serde::Deserialize)]
pub struct ListRequestsArgs {
    pub since: Option<String>,
    pub until: Option<String>,
    pub route: Option<String>,
    pub model: Option<String>,
    pub virtual_key_label: Option<String>,
    pub real_key_label: Option<String>,
    pub session_id: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_requests(state: State<'_, AppState>, args: ListRequestsArgs) -> Result<(Vec<RequestSummary>, i64), String> {
    let conn = state.store.read_conn();
    tokio::task::spawn_blocking(move || {
        let since = match args.since {
            Some(s) if !s.is_empty() => Some(parse_ts(&s)?),
            _ => None,
        };
        let until = parse_until(&args.until)?;
        let filter = RequestFilter {
            since,
            until,
            route: args.route,
            model: args.model,
            virtual_key_label: args.virtual_key_label,
            real_key_label: args.real_key_label,
            session_id: args.session_id,
        };
        let conn = conn.lock().expect("read connection mutex poisoned");
        store::list_requests(&conn, &filter, args.limit, args.offset).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_request(state: State<'_, AppState>, id: i64) -> Result<Option<Record>, String> {
    let conn = state.store.read_conn();
    tokio::task::spawn_blocking(move || {
        let conn = conn.lock().expect("read connection mutex poisoned");
        store::get_request(&conn, id).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_distinct_models(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let conn = state.store.read_conn();
    tokio::task::spawn_blocking(move || {
        let conn = conn.lock().expect("read connection mutex poisoned");
        store::distinct_models(&conn).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_distinct_virtual_key_labels(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let conn = state.store.read_conn();
    tokio::task::spawn_blocking(move || {
        let conn = conn.lock().expect("read connection mutex poisoned");
        store::distinct_virtual_key_labels(&conn).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_distinct_session_ids(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let conn = state.store.read_conn();
    tokio::task::spawn_blocking(move || {
        let conn = conn.lock().expect("read connection mutex poisoned");
        store::distinct_session_ids(&conn).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[derive(Serialize)]
pub struct GatewayStatus {
    pub listen_addr: String,
    pub config_path: String,
    pub db_path: String,
    pub bind_error: Option<String>,
    pub tls_enabled: bool,
    /// `https://localhost:{port}` when TLS is on, for use in a browser or
    /// client SDK on this machine.
    pub https_url: Option<String>,
    /// `https://{mdns_hostname}.local:{port}` when both TLS and mDNS are
    /// on, for reaching the gateway from elsewhere on the local network.
    pub mdns_url: Option<String>,
    /// SHA-256 fingerprint of the self-signed cert, so the user can
    /// manually verify/trust it in a browser.
    pub tls_fingerprint: Option<String>,
}

#[tauri::command(rename_all = "snake_case")]
pub fn get_gateway_status(state: State<AppState>) -> GatewayStatus {
    let cfg = state.config.lock().expect("config mutex poisoned");
    let port = cfg.listen.rsplit(':').next().and_then(|p| p.parse::<u16>().ok());

    let https_url = match (cfg.tls_enabled, port) {
        (true, Some(port)) => Some(format!("https://localhost:{port}")),
        _ => None,
    };
    let mdns_url = match (cfg.tls_enabled, port, cfg.mdns_hostname.is_empty()) {
        (true, Some(port), false) => Some(format!("https://{}.local:{port}", cfg.mdns_hostname)),
        _ => None,
    };

    GatewayStatus {
        listen_addr: cfg.listen.clone(),
        config_path: state.config_path.to_string_lossy().to_string(),
        db_path: cfg.db_path.clone(),
        bind_error: state.bind_error.lock().expect("bind_error mutex poisoned").clone(),
        tls_enabled: cfg.tls_enabled,
        https_url,
        mdns_url,
        tls_fingerprint: state.tls_fingerprint.clone(),
    }
}
