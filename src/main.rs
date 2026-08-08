use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use clap::Parser;
use tollgate::shutdown::{drain_with_timeout, normalize_listen};
use tollgate::{config, gateway, store};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::oneshot;

mod cli;

/// A local LLM gateway: sits in front of OpenAI/Anthropic-compatible
/// client SDKs, forwards requests to the real upstream unmodified, and
/// logs usage/latency/cost/full request-response content to a local
/// SQLite file.
///
/// Run with no subcommand to start the gateway server. Use `routes`/
/// `keys`/`real-keys`/`pricing` to script config.yaml edits instead of
/// hand-editing it — an already-running standalone server picks up the
/// change via the existing SIGHUP reload mechanism (the Tauri app does
/// not watch the file, so it needs a restart instead).
#[derive(Parser)]
#[command(name = "tollgate")]
struct Args {
    /// Path to the YAML config file. Defaults to the same OS-standard
    /// per-app config directory the Tauri desktop app uses, so the CLI
    /// and the app manage the same file out of the box.
    #[arg(long, default_value_os_t = config::default_config_path(), global = true)]
    config: PathBuf,
    #[command(subcommand)]
    command: Option<cli::Command>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")))
        .init();

    let args = Args::parse();

    if let Some(command) = args.command {
        return cli::run(command, &args.config);
    }

    let cfg = config::load(&args.config)?;
    tracing::info!(config = %args.config.display(), "config loaded");

    let store = Arc::new(store::Store::open(&cfg.db_path)?);
    let gw = gateway::Gateway::new(&cfg, store.clone())?;

    let listener = tokio::net::TcpListener::bind(normalize_listen(&cfg.listen)).await?;
    tracing::info!(listen = %cfg.listen, "gateway listening");

    let shutdown_timeout = Arc::new(Mutex::new(cfg.shutdown_timeout.clone()));
    spawn_reload_on_sighup(gw.clone(), args.config.clone(), shutdown_timeout.clone());

    let router = gateway::router(gw);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
    });

    wait_for_shutdown_signal().await;
    tracing::info!("shutdown signal received, no longer accepting new requests");
    let _ = shutdown_tx.send(());

    let timeout = shutdown_timeout.lock().expect("shutdown_timeout mutex poisoned").clone();
    drain_with_timeout(server_handle, &timeout).await?;

    // The server task (and the Arc<Gateway> it held) is gone once
    // drain_with_timeout returns; this is the last strong reference to
    // the store, so dropping it here waits for the writer thread to
    // drain its queue and closes the DB connection.
    drop(store);
    tracing::info!("shut down cleanly");
    Ok(())
}

/// Waits for SIGTERM or Ctrl-C (SIGINT).
async fn wait_for_shutdown_signal() {
    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = sigterm.recv() => {},
        _ = tokio::signal::ctrl_c() => {},
    }
}

/// Watches for SIGHUP and reloads the config file in place on each one.
/// Only routes/keys/max_body_bytes/pricing/shutdown_timeout take effect;
/// `listen`/`db_path` changes require a restart, same as the config
/// file's own [`config::Config`] semantics.
fn spawn_reload_on_sighup(gw: Arc<gateway::Gateway>, config_path: PathBuf, shutdown_timeout: Arc<Mutex<String>>) {
    tokio::spawn(async move {
        let mut hangup = match signal(SignalKind::hangup()) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("failed to install SIGHUP handler: {e}");
                return;
            }
        };
        loop {
            if hangup.recv().await.is_none() {
                return;
            }
            match config::load(&config_path) {
                Ok(new_cfg) => match gw.reload(&new_cfg) {
                    Ok(()) => {
                        *shutdown_timeout.lock().expect("shutdown_timeout mutex poisoned") = new_cfg.shutdown_timeout.clone();
                        tracing::info!("config reloaded");
                    }
                    Err(e) => tracing::error!("reload failed, keeping previous config: {e}"),
                },
                Err(e) => tracing::error!("reload failed, keeping previous config: {e}"),
            }
        }
    });
}
