//! Shared app state, injected via `app.manage()` and reachable from every
//! `#[tauri::command]` as `State<AppState>`: the same `Arc<Gateway>`/
//! `Arc<Store>` the embedded axum server uses, plus everything needed to
//! save config changes and drain the server gracefully on quit.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tollgate::config::{self, Config};
use tollgate::gateway::Gateway;
use tollgate::mdns::MdnsHandle;
use tollgate::shutdown::ShutdownHandle;
use tollgate::store::Store;

pub struct AppState {
    pub gateway: Arc<Gateway>,
    pub store: Arc<Store>,
    pub config: Mutex<Config>,
    pub config_path: PathBuf,
    /// Set if the embedded axum server failed to bind its listener; the
    /// frontend surfaces this as a banner instead of the app just
    /// crashing or silently not serving traffic.
    pub bind_error: Arc<Mutex<Option<String>>>,
    /// Consumed once, by [`AppState::graceful_shutdown`], to tell the
    /// embedded server to stop accepting new connections.
    pub shutdown_handle: Mutex<Option<ShutdownHandle>>,
    pub server_handle: Mutex<Option<tokio::task::JoinHandle<std::io::Result<()>>>>,
    /// Consumed once, by [`AppState::graceful_shutdown`], to unregister the
    /// mDNS advertisement before the process exits. `None` when mDNS is
    /// disabled or failed to start.
    pub mdns_handle: Mutex<Option<MdnsHandle>>,
    /// SHA-256 fingerprint of the self-signed TLS cert, surfaced to the
    /// frontend so the user can manually verify/trust it. `None` when
    /// `tls_enabled` is false.
    pub tls_fingerprint: Option<String>,
}

/// Everything about the embedded server's running instance, grouped into
/// one struct purely to keep clippy's `too_many_arguments` happy on
/// [`AppState::new`] (same rationale as `commands::ListRequestsArgs`).
pub struct ServerHandles {
    pub bind_error: Arc<Mutex<Option<String>>>,
    pub shutdown_handle: ShutdownHandle,
    pub server_handle: tokio::task::JoinHandle<std::io::Result<()>>,
    pub mdns_handle: Option<MdnsHandle>,
    pub tls_fingerprint: Option<String>,
}

impl AppState {
    pub fn new(gateway: Arc<Gateway>, store: Arc<Store>, config: Config, config_path: PathBuf, server: ServerHandles) -> Self {
        AppState {
            gateway,
            store,
            config: Mutex::new(config),
            config_path,
            bind_error: server.bind_error,
            shutdown_handle: Mutex::new(Some(server.shutdown_handle)),
            server_handle: Mutex::new(Some(server.server_handle)),
            mdns_handle: Mutex::new(server.mdns_handle),
            tls_fingerprint: server.tls_fingerprint,
        }
    }

    /// Applies `f` to a clone of the current config, validates the
    /// result structurally (routes may still be empty/half-built — see
    /// [`config::validate_routes`]), and only on success persists it to
    /// disk, hot-reloads the gateway, and commits it as the new current
    /// config. A validation/save/reload failure leaves the previously
    /// committed config untouched.
    pub fn update_config(&self, f: impl FnOnce(&mut Config)) -> anyhow::Result<()> {
        let mut cfg = self.config.lock().expect("config mutex poisoned").clone();
        f(&mut cfg);
        config::validate_routes(&cfg)?;
        config::save(&self.config_path, &cfg)?;
        self.gateway.reload(&cfg)?;
        *self.config.lock().expect("config mutex poisoned") = cfg;
        Ok(())
    }

    /// Stops the embedded server from accepting new connections, waits
    /// for in-flight requests to drain (capped by `shutdown_timeout`),
    /// then closes the store's writer thread. Idempotent — safe to call
    /// more than once, only the first call does anything.
    pub fn graceful_shutdown(&self) {
        let timeout = self.config.lock().expect("config mutex poisoned").shutdown_timeout.clone();
        if let Some(shutdown_handle) = self.shutdown_handle.lock().expect("shutdown_handle mutex poisoned").take() {
            shutdown_handle.send(humantime::parse_duration(&timeout).ok());
        }
        if let Some(handle) = self.server_handle.lock().expect("server_handle mutex poisoned").take() {
            tauri::async_runtime::block_on(async move {
                if let Err(e) = tollgate::shutdown::drain_with_timeout(handle, &timeout).await {
                    tracing::error!("gateway drain failed: {e}");
                }
            });
        }
        if let Some(mdns) = self.mdns_handle.lock().expect("mdns_handle mutex poisoned").take() {
            mdns.stop();
        }
        self.store.close();
    }
}
