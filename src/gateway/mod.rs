//! The reverse-proxy handler: matches an inbound request to a configured
//! upstream route, forwards it verbatim, captures usage/latency/content
//! along the way, and relays the upstream response back to the client
//! unmodified.

mod boundedbuffer;

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use arc_swap::ArcSwap;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use boundedbuffer::BoundedBuffer;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::config::{self, Config};
use crate::format::{self, Adapter};
use crate::pricing::{self, Table as PricingTable};
use crate::store::{Record, Store};

struct CompiledRoute {
    name: String,
    format: String,
    adapter: Box<dyn Adapter>,
    upstream: reqwest::Url,
}

/// What a single configured virtual key resolves to: the route it
/// belongs to, the labels to record in the usage log (for both the
/// virtual key the client used and the real key it maps to), and the
/// real key's value to inject when forwarding upstream. Neither the
/// virtual key's value nor the real key's value is ever stored anywhere
/// past this lookup.
struct BoundKey {
    route: Arc<CompiledRoute>,
    virtual_label: String,
    real_key_label: String,
    real_key_value: String,
}

/// Routes requests to configured upstreams and logs each call. The
/// route table sits behind an [`ArcSwap`] so [`Gateway::reload`] can
/// swap it in without any locking on the request path: in-flight
/// requests keep using whichever table they already loaded, new
/// requests see the update as soon as it's stored.
pub struct Gateway {
    by_key: ArcSwap<HashMap<String, Arc<BoundKey>>>,
    max_body_bytes: AtomicI64,
    pricing: ArcSwap<PricingTable>,
    store: Arc<Store>,
    client: reqwest::Client,
}

impl Gateway {
    pub fn new(cfg: &Config, store: Arc<Store>) -> anyhow::Result<Arc<Gateway>> {
        let gw = Arc::new(Gateway {
            by_key: ArcSwap::from_pointee(HashMap::new()),
            max_body_bytes: AtomicI64::new(config::DEFAULT_MAX_BODY_BYTES),
            pricing: ArcSwap::from_pointee(PricingTable::new()),
            store,
            // No overall timeout: LLM responses (especially long
            // streams) can legitimately take minutes. A slow/dead
            // upstream is the client's problem to time out on, same as
            // if they'd hit the API directly.
            client: reqwest::Client::builder().build()?,
        });
        gw.reload(cfg)?;
        Ok(gw)
    }

    /// Rebuilds the route table from `cfg` and atomically swaps it in.
    /// Only routes/keys/max_body_bytes/pricing take effect; `cfg.listen`
    /// and `cfg.db_path` are ignored here since changing the listener or
    /// DB file requires a restart.
    pub fn reload(&self, cfg: &Config) -> anyhow::Result<()> {
        let mut by_key = HashMap::new();
        for r in &cfg.routes {
            let upstream = reqwest::Url::parse(&r.upstream)
                .with_context(|| format!("route {:?}: invalid upstream {:?}", r.name, r.upstream))?;
            let adapter = format::adapter_for(&r.format)
                .ok_or_else(|| anyhow::anyhow!("route {:?}: unknown format {:?}", r.name, r.format))?;
            let compiled = Arc::new(CompiledRoute {
                name: r.name.clone(),
                format: r.format.clone(),
                adapter,
                upstream,
            });

            let real_key_values: HashMap<&str, &str> =
                r.real_keys.iter().map(|rk| (rk.label.as_str(), rk.value.as_str())).collect();

            for k in &r.keys {
                let label = if k.label.is_empty() { r.name.clone() } else { k.label.clone() };
                by_key.insert(
                    k.value.clone(),
                    Arc::new(BoundKey {
                        route: compiled.clone(),
                        virtual_label: label,
                        real_key_label: k.real_key.clone(),
                        real_key_value: real_key_values.get(k.real_key.as_str()).copied().unwrap_or("").to_string(),
                    }),
                );
            }
        }
        self.by_key.store(Arc::new(by_key));
        self.max_body_bytes.store(cfg.max_body_bytes, Ordering::Relaxed);
        self.pricing.store(Arc::new(cfg.pricing.clone()));
        Ok(())
    }

    fn log(&self, record: Record) {
        self.store.insert(record);
    }
}

pub fn router(gw: Arc<Gateway>) -> Router {
    Router::new()
        .route("/_stats", get(handle_stats))
        .fallback(any(handle_proxy))
        .with_state(gw)
}

/// Pulls the client's API key out of the request the same way OpenAI's
/// and Anthropic's own servers would read it: "Authorization: Bearer
/// <key>" first, falling back to "x-api-key". An absent header yields
/// "", which routes can explicitly opt into via `keys: [""]`.
fn extract_key(headers: &HeaderMap) -> String {
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        return auth.strip_prefix("Bearer ").unwrap_or(auth).to_string();
    }
    headers.get("x-api-key").and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
}

/// Reads the client-supplied `x-session-id` header, if any, so the UI
/// can group requests that belong to the same conversation. There is
/// no server-side heuristic fallback — an absent header yields "".
fn extract_session_id(headers: &HeaderMap) -> String {
    headers.get("x-session-id").and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
}

const HOP_BY_HOP_HEADERS: [&str; 8] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Strips the standard hop-by-hop headers, plus any extra header names
/// listed inside a `Connection` header value (per RFC 7230 §6.1, a
/// connection can nominate additional headers as hop-by-hop for that
/// leg only).
fn remove_hop_by_hop_headers(headers: &mut HeaderMap) {
    let extra: Vec<String> = headers
        .get_all("connection")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(',').map(|s| s.trim().to_string()))
        .collect();
    for name in extra {
        if let Ok(header_name) = HeaderName::try_from(name.as_str()) {
            headers.remove(header_name);
        }
    }
    for name in HOP_BY_HOP_HEADERS {
        headers.remove(name);
    }
}

/// Overwrites whichever header the client used to authenticate
/// (Authorization or x-api-key, matching [`extract_key`]'s own
/// precedence) with the real upstream credential, so the client's
/// virtual key is never sent to the provider.
fn inject_upstream_key(headers: &mut HeaderMap, real_value: &str) {
    if headers.contains_key("authorization") {
        if let Ok(v) = HeaderValue::from_str(&format!("Bearer {real_value}")) {
            headers.insert(HeaderName::from_static("authorization"), v);
        }
        return;
    }
    if let Ok(v) = HeaderValue::from_str(real_value) {
        headers.insert(HeaderName::from_static("x-api-key"), v);
    }
}

/// Joins an upstream base path (often empty) with the inbound request
/// path, forwarded verbatim, ensuring exactly one slash between them.
fn join_target_url(upstream: &reqwest::Url, path: &str, query: Option<&str>) -> reqwest::Url {
    let mut url = upstream.clone();
    let base = upstream.path().trim_end_matches('/');
    let rest = if path.is_empty() {
        "/".to_string()
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    url.set_path(&format!("{base}{rest}"));
    url.set_query(query);
    url
}

enum ReadBodyError {
    TooLarge,
    Other(axum::Error),
}

/// Reads `body` into memory, aborting as soon as more than `limit` bytes
/// have arrived rather than buffering an unbounded amount first — the
/// async equivalent of Go's `http.MaxBytesReader`.
async fn read_capped(body: Body, limit: usize) -> Result<Bytes, ReadBodyError> {
    let mut stream = body.into_data_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(ReadBodyError::Other)?;
        if buf.len() + chunk.len() > limit {
            return Err(ReadBodyError::TooLarge);
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(buf))
}

fn copy_response_headers(builder: axum::http::response::Builder, headers: &reqwest::header::HeaderMap) -> axum::http::response::Builder {
    let mut builder = builder;
    for (name, value) in headers.iter() {
        builder = builder.header(name, value);
    }
    builder
}

/// Renders the inbound request line plus headers as raw HTTP text, for
/// the request browser's "Raw HTTP" view. Must be called on the
/// original `headers` (before [`inject_upstream_key`] overwrites the
/// client's virtual key with the real upstream credential), so the real
/// secret never appears in what gets formatted here.
fn format_request_head(method: &Method, uri: &Uri, headers: &HeaderMap) -> String {
    let mut out = format!("{method} {uri} HTTP/1.1\n");
    for (name, value) in headers.iter() {
        out.push_str(&format!("{name}: {}\n", value.to_str().unwrap_or("<binary>")));
    }
    out
}

/// Renders the upstream status line plus response headers as raw HTTP
/// text, for the request browser's "Raw HTTP" view.
fn format_response_head(status: StatusCode, headers: &reqwest::header::HeaderMap) -> String {
    let mut out = format!("HTTP/1.1 {} {}\n", status.as_u16(), status.canonical_reason().unwrap_or(""));
    for (name, value) in headers.iter() {
        out.push_str(&format!("{name}: {}\n", value.to_str().unwrap_or("<binary>")));
    }
    out
}

async fn handle_proxy(State(gw): State<Arc<Gateway>>, method: Method, uri: Uri, headers: HeaderMap, body: Body) -> Response {
    let key = extract_key(&headers);
    let request_headers = format_request_head(&method, &uri, &headers);
    let bound = {
        let guard = gw.by_key.load();
        guard.get(&key).cloned()
    };
    let Some(bound) = bound else {
        return (StatusCode::UNAUTHORIZED, "no route configured for this API key").into_response();
    };
    let rt = bound.route.clone();

    let start = Instant::now();
    let started_at = Utc::now();

    let max_body = gw.max_body_bytes.load(Ordering::Relaxed);
    let req_body = match read_capped(body, max_body.max(0) as usize).await {
        Ok(b) => b,
        Err(ReadBodyError::TooLarge) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("request body exceeds {max_body} byte limit (max_body_bytes in config.yaml)"),
            )
                .into_response();
        }
        Err(ReadBodyError::Other(e)) => {
            return (StatusCode::BAD_REQUEST, format!("read request body: {e}")).into_response();
        }
    };

    let parsed = rt.adapter.parse_request(&req_body);
    let path = uri.path().to_string();

    let target_url = join_target_url(&rt.upstream, &path, uri.query());

    let mut out_headers = headers.clone();
    remove_hop_by_hop_headers(&mut out_headers);
    inject_upstream_key(&mut out_headers, &bound.real_key_value);
    // The client's own Host header (naming this gateway) must not be
    // forwarded — reqwest/hyper otherwise send it verbatim instead of
    // deriving one from target_url, which upstreams (and anything
    // routing on Host) rightly reject. Removing it lets the HTTP client
    // set the correct Host for target_url itself.
    out_headers.remove("host");

    let reqwest_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
        Ok(m) => m,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("build upstream request: {e}")).into_response(),
    };

    let resp = match gw
        .client
        .request(reqwest_method, target_url)
        .headers(out_headers)
        .body(req_body.clone())
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("upstream request failed: {e:?}");
            gw.log(Record {
                timestamp: started_at,
                route: rt.name.clone(),
                virtual_key_label: bound.virtual_label.clone(),
                real_key_label: bound.real_key_label.clone(),
                format: rt.format.clone(),
                model: parsed.model.clone(),
                path,
                stream: parsed.stream,
                latency_ms: start.elapsed().as_millis() as i64,
                request_body: String::from_utf8_lossy(&req_body).to_string(),
                error: e.to_string(),
                virtual_key_value: key,
                session_id: extract_session_id(&headers),
                request_headers,
                ..Default::default()
            });
            return (StatusCode::BAD_GATEWAY, format!("upstream request failed: {e}")).into_response();
        }
    };

    let status = resp.status();
    let mut resp_headers = resp.headers().clone();
    remove_reqwest_hop_by_hop_headers(&mut resp_headers);
    let response_headers = format_response_head(status, &resp_headers);

    let content_type = resp_headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("");
    let is_stream = parsed.stream && content_type.contains("text/event-stream");

    let response_builder = copy_response_headers(Response::builder().status(status), &resp_headers);

    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(16);
    let gw2 = gw.clone();
    let bound2 = bound.clone();
    let rt2 = rt.clone();
    let req_body_str = String::from_utf8_lossy(&req_body).to_string();
    let model = parsed.model.clone();
    let stream_flag = parsed.stream;
    let virtual_key_value = key;
    let session_id = extract_session_id(&headers);

    tokio::spawn(async move {
        let mut upstream_stream = resp.bytes_stream();
        let mut agg = if is_stream { Some(rt2.adapter.new_stream_aggregator()) } else { None };
        let mut capture = BoundedBuffer::new(max_body.max(0) as usize);
        let mut upstream_err: Option<String> = None;

        while let Some(chunk) = upstream_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if let Some(agg) = agg.as_mut() {
                        agg.feed(&bytes);
                    }
                    capture.write(&bytes);
                    if tx.send(Ok(bytes)).await.is_err() {
                        break; // client disconnected
                    }
                }
                Err(e) => {
                    upstream_err = Some(e.to_string());
                    let _ = tx.send(Err(std::io::Error::other(e))).await;
                    break;
                }
            }
        }

        // response_body always holds the raw bytes actually relayed to
        // the client (SSE frames and all, for streamed responses) — usage
        // is extracted separately, from the aggregator for streamed
        // responses or by parsing the raw body directly otherwise.
        let raw = capture.bytes();
        let usage = if let Some(agg) = agg {
            agg.finish()
        } else if status.as_u16() < 400 {
            rt2.adapter.parse_response(raw)
        } else {
            format::Usage::default()
        };
        let mut response_body = String::from_utf8_lossy(raw).to_string();
        if capture.truncated() {
            response_body.push_str(&format!(
                "\n...[truncated: response body exceeded the {max_body} byte capture limit; the full response was still relayed to the client]"
            ));
        }

        let cost = pricing::cost_usd(&model, &usage, &gw2.pricing.load());
        let error = upstream_err.unwrap_or_else(|| {
            if status.as_u16() >= 400 {
                format!("upstream returned status {}", status.as_u16())
            } else {
                String::new()
            }
        });

        gw2.log(Record {
            timestamp: started_at,
            route: rt2.name.clone(),
            virtual_key_label: bound2.virtual_label.clone(),
            real_key_label: bound2.real_key_label.clone(),
            format: rt2.format.clone(),
            model,
            path,
            stream: stream_flag,
            status_code: status.as_u16() as i64,
            latency_ms: start.elapsed().as_millis() as i64,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
            cache_creation_tokens: usage.cache_creation_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cost_usd: cost,
            request_body: req_body_str,
            response_body,
            error,
            virtual_key_value,
            session_id,
            request_headers,
            response_headers,
            ..Default::default()
        });
    });

    let body = Body::from_stream(ReceiverStream::new(rx));
    match response_builder.body(body) {
        Ok(r) => r,
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("build response: {e}")).into_response(),
    }
}

fn remove_reqwest_hop_by_hop_headers(headers: &mut reqwest::header::HeaderMap) {
    let extra: Vec<String> = headers
        .get_all("connection")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(',').map(|s| s.trim().to_string()))
        .collect();
    for name in extra {
        if let Ok(header_name) = reqwest::header::HeaderName::try_from(name.as_str()) {
            headers.remove(header_name);
        }
    }
    for name in HOP_BY_HOP_HEADERS {
        headers.remove(name);
    }
}

#[derive(Deserialize)]
struct StatsQuery {
    group_by: Option<String>,
    since: Option<String>,
}

fn parse_since(raw: Option<&str>) -> anyhow::Result<DateTime<Utc>> {
    let dur = match raw {
        Some(s) if !s.is_empty() => humantime::parse_duration(s)?,
        _ => std::time::Duration::from_secs(24 * 3600),
    };
    let delta = chrono::TimeDelta::from_std(dur).unwrap_or(chrono::TimeDelta::hours(24));
    Ok(Utc::now() - delta)
}

async fn handle_stats(State(gw): State<Arc<Gateway>>, Query(q): Query<StatsQuery>) -> Response {
    let group_by = q.group_by.unwrap_or_else(|| "model".to_string());
    let since = match parse_since(q.since.as_deref()) {
        Ok(d) => d,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("invalid since duration: {e}")).into_response(),
    };

    let conn = gw.store.read_conn();
    let result = tokio::task::spawn_blocking(move || {
        let conn = conn.lock().expect("store read connection mutex poisoned");
        crate::store::stats(&conn, &group_by, since, None)
    })
    .await;

    match result {
        Ok(Ok(rows)) => axum::Json(rows).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_key_prefers_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer abc"));
        assert_eq!(extract_key(&headers), "abc");
    }

    #[test]
    fn extract_key_falls_back_to_x_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("xyz"));
        assert_eq!(extract_key(&headers), "xyz");
    }

    #[test]
    fn extract_key_empty_when_no_header() {
        let headers = HeaderMap::new();
        assert_eq!(extract_key(&headers), "");
    }

    #[test]
    fn extract_session_id_reads_header() {
        let mut headers = HeaderMap::new();
        headers.insert("x-session-id", HeaderValue::from_static("sess-123"));
        assert_eq!(extract_session_id(&headers), "sess-123");
    }

    #[test]
    fn extract_session_id_empty_when_no_header() {
        let headers = HeaderMap::new();
        assert_eq!(extract_session_id(&headers), "");
    }

    #[test]
    fn join_target_url_appends_path_and_query() {
        let upstream = reqwest::Url::parse("https://api.openai.com").unwrap();
        let url = join_target_url(&upstream, "/v1/chat/completions", Some("foo=bar"));
        assert_eq!(url.as_str(), "https://api.openai.com/v1/chat/completions?foo=bar");
    }

    #[test]
    fn join_target_url_handles_upstream_with_base_path() {
        let upstream = reqwest::Url::parse("https://api.example.com/base/").unwrap();
        let url = join_target_url(&upstream, "/v1/x", None);
        assert_eq!(url.as_str(), "https://api.example.com/base/v1/x");
    }

    #[test]
    fn inject_upstream_key_prefers_authorization() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer client-key"));
        inject_upstream_key(&mut headers, "real-key");
        assert_eq!(headers.get("authorization").unwrap(), "Bearer real-key");
    }

    #[test]
    fn inject_upstream_key_falls_back_to_x_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("client-key"));
        inject_upstream_key(&mut headers, "real-key");
        assert_eq!(headers.get("x-api-key").unwrap(), "real-key");
    }

    #[test]
    fn format_request_head_renders_request_line_and_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer vk-abc"));
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        let uri: Uri = "/v1/chat/completions?foo=bar".parse().unwrap();
        let out = format_request_head(&Method::POST, &uri, &headers);
        assert!(out.starts_with("POST /v1/chat/completions?foo=bar HTTP/1.1\n"));
        assert!(out.contains("authorization: Bearer vk-abc\n"));
        assert!(out.contains("content-type: application/json\n"));
    }

    #[test]
    fn format_response_head_renders_status_line_and_headers() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("content-type", reqwest::header::HeaderValue::from_static("text/event-stream"));
        let out = format_response_head(StatusCode::OK, &headers);
        assert!(out.starts_with("HTTP/1.1 200 OK\n"));
        assert!(out.contains("content-type: text/event-stream\n"));
    }

    #[test]
    fn remove_hop_by_hop_headers_strips_standard_and_nominated() {
        let mut headers = HeaderMap::new();
        headers.insert("connection", HeaderValue::from_static("keep-alive, x-custom"));
        headers.insert("keep-alive", HeaderValue::from_static("timeout=5"));
        headers.insert("x-custom", HeaderValue::from_static("val"));
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        remove_hop_by_hop_headers(&mut headers);
        assert!(!headers.contains_key("connection"));
        assert!(!headers.contains_key("keep-alive"));
        assert!(!headers.contains_key("x-custom"));
        assert!(headers.contains_key("content-type"));
    }
}
