//! Graceful-shutdown helpers shared by the CLI binary and the Tauri app:
//! both start an axum server on a background task and need to wait for it
//! to drain in-flight requests, capped by an optional Go-style duration
//! string ("30s", "1m"; empty means wait indefinitely).

use std::net::SocketAddr;

/// Triggers graceful shutdown of whichever server task is running: plain
/// `axum::serve` (HTTP) signals via a `oneshot`, TLS via `axum-server`
/// signals through its own `Handle`. Letting both entrypoints hold one type
/// regardless of which listener is active keeps the rest of the shutdown
/// sequence (this module's [`drain_with_timeout`]) identical either way.
pub enum ShutdownHandle {
    Oneshot(tokio::sync::oneshot::Sender<()>),
    AxumServer(axum_server::Handle<SocketAddr>),
}

impl ShutdownHandle {
    /// Signals the server to stop accepting new connections and start
    /// draining in-flight ones. `timeout` is only consulted by the
    /// `AxumServer` variant (its `Handle` accepts a grace period directly);
    /// the `Oneshot`/plain-HTTP path relies entirely on the outer
    /// [`drain_with_timeout`] call for timing out.
    pub fn send(self, timeout: Option<std::time::Duration>) {
        match self {
            Self::Oneshot(tx) => {
                let _ = tx.send(());
            }
            Self::AxumServer(handle) => handle.graceful_shutdown(timeout),
        }
    }
}

/// Waits for `server_handle` to finish draining in-flight requests, capped
/// at `shutdown_timeout` (read once, at the moment the caller decided to
/// shut down). Empty means wait indefinitely. An unparseable value is
/// treated the same as empty, with a warning, rather than failing the
/// whole shutdown.
pub async fn drain_with_timeout(server_handle: tokio::task::JoinHandle<std::io::Result<()>>, shutdown_timeout: &str) -> anyhow::Result<()> {
    if shutdown_timeout.is_empty() {
        tracing::info!("no shutdown_timeout configured — waiting for all in-flight requests to finish");
        server_handle.await??;
        return Ok(());
    }

    match humantime::parse_duration(shutdown_timeout) {
        Ok(d) => {
            tracing::info!(timeout = ?d, "waiting for in-flight requests (shutdown_timeout)");
            match tokio::time::timeout(d, server_handle).await {
                Ok(joined) => joined??,
                Err(_) => tracing::warn!("shutdown_timeout elapsed; exiting with requests possibly still in flight"),
            }
        }
        Err(e) => {
            tracing::warn!("invalid shutdown_timeout {shutdown_timeout:?} ({e}); waiting indefinitely instead");
            server_handle.await??;
        }
    }
    Ok(())
}

/// `":8787"`-style listen strings (the convention `net/http`-style tools
/// use for "all interfaces") need an explicit host for Rust's
/// `ToSocketAddrs`.
pub fn normalize_listen(listen: &str) -> String {
    if let Some(port) = listen.strip_prefix(':') {
        format!("0.0.0.0:{port}")
    } else {
        listen.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_listen_expands_bare_port() {
        assert_eq!(normalize_listen(":8787"), "0.0.0.0:8787");
    }

    #[test]
    fn normalize_listen_leaves_explicit_host_alone() {
        assert_eq!(normalize_listen("127.0.0.1:8787"), "127.0.0.1:8787");
    }
}
