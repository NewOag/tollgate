//! Tauri host: runs the same `tollgate` gateway/store/config code the CLI
//! binary uses, in-process, and exposes it to the Svelte frontend via
//! `#[tauri::command]`s that read/write the same `Arc<Gateway>`/
//! `Arc<Store>` the embedded axum server serves real proxy traffic on.

mod commands;
mod state;
mod tray;

use std::sync::{Arc, Mutex};

use tollgate::{config, gateway, shutdown, store};
use state::AppState;
use tauri::Manager;
use tokio::sync::oneshot;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_handle = app.handle().clone();

            // Every-user-stable config/db location, rather than relying on
            // an unpredictable current working directory.
            let app_dir = app_handle.path().app_config_dir()?;
            std::fs::create_dir_all(&app_dir)?;
            let config_path = app_dir.join("config.yaml");

            let mut cfg = config::load_or_init(&config_path)?;
            if cfg.db_path.is_empty() || cfg.db_path == config::DEFAULT_DB_PATH {
                cfg.db_path = app_dir.join("tollgate.db").to_string_lossy().to_string();
            }

            let store = Arc::new(store::Store::open(&cfg.db_path)?);
            let gw = gateway::Gateway::new(&cfg, store.clone())?;
            let router = gateway::router(gw.clone());

            let listen = shutdown::normalize_listen(&cfg.listen);
            let bind_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
            let bind_error_task = bind_error.clone();

            let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
            let tokio_handle = tauri::async_runtime::handle().inner().clone();
            let server_handle = tokio_handle.spawn(async move {
                let listener = match tokio::net::TcpListener::bind(&listen).await {
                    Ok(l) => l,
                    Err(e) => {
                        let msg = format!("failed to bind {listen}: {e}");
                        tracing::error!("{msg}");
                        *bind_error_task.lock().expect("bind_error mutex poisoned") = Some(msg);
                        return Ok(());
                    }
                };
                tracing::info!(listen = %listen, "gateway listening");
                axum::serve(listener, router)
                    .with_graceful_shutdown(async move {
                        let _ = shutdown_rx.await;
                    })
                    .await
            });

            app.manage(AppState::new(gw, store, cfg, config_path, bind_error, shutdown_tx, server_handle));

            tray::setup(&app_handle)?;

            if let Some(main_window) = app.get_webview_window("main") {
                let main_window_for_event = main_window.clone();
                main_window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        tray::hide_instead_of_close(&main_window_for_event, api);
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_route,
            commands::delete_route,
            commands::save_pricing,
            commands::delete_pricing,
            commands::save_general_settings,
            commands::get_stats,
            commands::get_time_series,
            commands::list_requests,
            commands::get_request,
            commands::list_distinct_models,
            commands::list_distinct_virtual_key_labels,
            commands::list_distinct_session_ids,
            commands::get_gateway_status,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                tray::handle_exit_requested(app_handle, &api);
            }
        });
}
