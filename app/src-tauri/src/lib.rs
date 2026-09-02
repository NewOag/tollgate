//! Tauri host: runs the same `tollgate` gateway/store/config code the CLI
//! binary uses, in-process, and exposes it to the Svelte frontend via
//! `#[tauri::command]`s that read/write the same `Arc<Gateway>`/
//! `Arc<Store>` the embedded axum server serves real proxy traffic on.

mod commands;
mod state;
mod tray;

use std::sync::{Arc, Mutex};

use tollgate::shutdown::ShutdownHandle;
use tollgate::{config, gateway, mdns, shutdown, store, tls};
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

            // Cert generation/loading is blocking file I/O but not async,
            // so it can run directly here rather than inside the spawned
            // task below — that keeps `AppState` (which needs the
            // fingerprint and a ShutdownHandle right away) fully built
            // before `app.manage(...)`.
            let tls_cert = if cfg.tls_enabled {
                let cert_dir = tls::resolve_cert_dir(&cfg, &config_path);
                let sans = tls::build_san_list(&cfg.mdns_hostname);
                match tls::load_or_generate(&cert_dir, &sans) {
                    Ok(c) => Some(c),
                    Err(e) => {
                        tracing::error!("TLS cert error, falling back to HTTP: {e}");
                        None
                    }
                }
            } else {
                None
            };
            let tls_fingerprint = tls_cert.as_ref().map(|c| c.fingerprint.clone());

            let tokio_handle = tauri::async_runtime::handle().inner().clone();
            let (server_handle, shutdown_handle) = if let Some(cert) = tls_cert {
                let handle = axum_server::Handle::new();
                let server_handle_clone = handle.clone();
                let jh = tokio_handle.spawn(async move {
                    let std_listener = match tokio::net::TcpListener::bind(&listen).await.and_then(|l| l.into_std()) {
                        Ok(l) => l,
                        Err(e) => {
                            let msg = format!("failed to bind {listen}: {e}");
                            tracing::error!("{msg}");
                            *bind_error_task.lock().expect("bind_error mutex poisoned") = Some(msg);
                            return Ok(());
                        }
                    };
                    let rustls_cfg = tls::rustls_config(&cert).await.map_err(std::io::Error::other)?;
                    tracing::info!(listen = %listen, fingerprint = %cert.fingerprint, "gateway listening (https)");
                    axum_server::from_tcp_rustls(std_listener, rustls_cfg)?
                        .handle(server_handle_clone)
                        .serve(router.into_make_service())
                        .await
                });
                (jh, ShutdownHandle::AxumServer(handle))
            } else {
                let (tx, rx) = oneshot::channel::<()>();
                let jh = tokio_handle.spawn(async move {
                    let listener = match tokio::net::TcpListener::bind(&listen).await {
                        Ok(l) => l,
                        Err(e) => {
                            let msg = format!("failed to bind {listen}: {e}");
                            tracing::error!("{msg}");
                            *bind_error_task.lock().expect("bind_error mutex poisoned") = Some(msg);
                            return Ok(());
                        }
                    };
                    tracing::info!(listen = %listen, "gateway listening (http)");
                    axum::serve(listener, router)
                        .with_graceful_shutdown(async move {
                            let _ = rx.await;
                        })
                        .await
                });
                (jh, ShutdownHandle::Oneshot(tx))
            };

            let mdns_port = shutdown::normalize_listen(&cfg.listen)
                .rsplit(':')
                .next()
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(0);
            let mdns_handle = mdns::start_advertising(&cfg.mdns_hostname, mdns_port).unwrap_or_else(|e| {
                tracing::warn!("mDNS advertisement failed to start, continuing without it: {e}");
                None
            });

            app.manage(AppState::new(
                gw,
                store,
                cfg,
                config_path,
                state::ServerHandles { bind_error, shutdown_handle, server_handle, mdns_handle, tls_fingerprint },
            ));

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
        .run(|app_handle, event| match event {
            tauri::RunEvent::ExitRequested { api, .. } => {
                tray::handle_exit_requested(app_handle, &api);
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { has_visible_windows, .. } => {
                if !has_visible_windows {
                    tray::show_main_window(app_handle);
                }
            }
            _ => {}
        });
}
