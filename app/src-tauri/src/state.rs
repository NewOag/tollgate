//! Shared app state, injected via `app.manage()` and reachable from every
//! `#[tauri::command]` as `State<AppState>`: the same `Arc<Gateway>`/
//! `Arc<Store>` the embedded axum server uses, plus everything needed to
//! save config changes and drain the server gracefully on quit.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tollgate::config::{self, Config};
use tollgate::gateway::Gateway;
use tollgate::store::Store;
use tokio::sync::oneshot;

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
    pub shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    pub server_handle: Mutex<Option<tokio::task::JoinHandle<std::io::Result<()>>>>,
}

impl AppState {
    pub fn new(
        gateway: Arc<Gateway>,
        store: Arc<Store>,
        config: Config,
        config_path: PathBuf,
        bind_error: Arc<Mutex<Option<String>>>,
        shutdown_tx: oneshot::Sender<()>,
        server_handle: tokio::task::JoinHandle<std::io::Result<()>>,
    ) -> Self {
        AppState {
            gateway,
            store,
            config: Mutex::new(config),
            config_path,
            bind_error,
            shutdown_tx: Mutex::new(Some(shutdown_tx)),
            server_handle: Mutex::new(Some(server_handle)),
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
        if let Some(tx) = self.shutdown_tx.lock().expect("shutdown_tx mutex poisoned").take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.server_handle.lock().expect("server_handle mutex poisoned").take() {
            let timeout = self.config.lock().expect("config mutex poisoned").shutdown_timeout.clone();
            tauri::async_runtime::block_on(async move {
                if let Err(e) = tollgate::shutdown::drain_with_timeout(handle, &timeout).await {
                    tracing::error!("gateway drain failed: {e}");
                }
            });
        }
        self.store.close();
    }
}
