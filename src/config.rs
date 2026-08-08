//! Route/key/pricing configuration, parsed from a YAML file and
//! hot-reloadable on SIGHUP.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_LISTEN: &str = ":8787";
pub const DEFAULT_DB_PATH: &str = "./tollgate.db";
pub const DEFAULT_MAX_BODY_BYTES: i64 = 25 * 1024 * 1024;

/// Must match `identifier` in `app/src-tauri/tauri.conf.json` — this is
/// what makes [`default_config_path`] resolve to the same file the Tauri
/// app reads/writes via `app_handle.path().app_config_dir()`, so the CLI
/// and the desktop app agree on "the config file" without either side
/// having to be told its path explicitly.
const APP_ID: &str = "com.tollgate.app";

/// The CLI's default `--config` path when not overridden: the same
/// OS-standard per-app config directory Tauri's `app_config_dir()`
/// resolves to (e.g. `~/Library/Application Support/com.tollgate.app/`
/// on macOS), not the current working directory. Falls back to
/// `./config.yaml` if the OS config directory can't be determined
/// (e.g. `$HOME` unset), matching this crate's pre-existing default.
pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .map(|dir| dir.join(APP_ID).join("config.yaml"))
        .unwrap_or_else(|| PathBuf::from("./config.yaml"))
}

/// One upstream provider credential registered under a route. `label`
/// is how a `KeyEntry::real_key` and stats grouping reference it; `value`
/// is the actual credential forwarded to the upstream.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RealKeyEntry {
    pub label: String,
    pub value: String,
}

/// One virtual API key a route accepts from clients. `value` is matched
/// verbatim against the client's Authorization/x-api-key header; `label`
/// is a non-sensitive alias for stats grouping (falls back to the route
/// name when empty); `real_key` names which of the route's `real_keys`
/// this virtual key resolves to when forwarding upstream.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct KeyEntry {
    pub value: String,
    #[serde(default)]
    pub label: String,
    pub real_key: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Route {
    pub name: String,
    /// "openai" or "anthropic".
    pub format: String,
    pub upstream: String,
    #[serde(default)]
    pub real_keys: Vec<RealKeyEntry>,
    #[serde(default)]
    pub keys: Vec<KeyEntry>,
}

/// USD-per-million-token rates for one model. `cached_input_per_mtok`/
/// `cache_write_per_mtok` left at zero mean "not priced separately" —
/// [`crate::pricing::cost_usd`] falls back to `input_per_mtok` for them.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct ModelPrice {
    #[serde(default)]
    pub input_per_mtok: f64,
    #[serde(default)]
    pub output_per_mtok: f64,
    #[serde(default)]
    pub cached_input_per_mtok: f64,
    #[serde(default)]
    pub cache_write_per_mtok: f64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub listen: String,
    #[serde(default)]
    pub db_path: String,
    #[serde(default)]
    pub routes: Vec<Route>,

    /// Caps how much of a request/response body is buffered in memory.
    /// Requests larger than this are rejected outright (413); responses
    /// are always relayed to the client in full, but only the first
    /// `max_body_bytes` are captured for usage-parsing/logging. Defaults
    /// to [`DEFAULT_MAX_BODY_BYTES`] when unset or non-positive.
    #[serde(default)]
    pub max_body_bytes: i64,

    /// Caps how long a graceful shutdown waits for in-flight requests
    /// before forcing an exit. Empty (the default) waits indefinitely.
    /// A Go-style duration string ("30s", "1m"). Hot-reloadable.
    #[serde(default)]
    pub shutdown_timeout: String,

    /// Model name (matched verbatim against the client-submitted value)
    /// -> USD-per-million-token rates, used to compute each request's
    /// cost at write time. Models with no entry are simply not costed —
    /// not an error. Hot-reloadable; only affects requests logged after
    /// the reload, past records keep the cost computed under the old
    /// price.
    #[serde(default)]
    pub pricing: HashMap<String, ModelPrice>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("read config: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse config: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("{0}")]
    Validation(String),
}

fn invalid(msg: impl Into<String>) -> ConfigError {
    ConfigError::Validation(msg.into())
}

fn apply_defaults(cfg: &mut Config) {
    if cfg.listen.is_empty() {
        cfg.listen = DEFAULT_LISTEN.to_string();
    }
    if cfg.db_path.is_empty() {
        cfg.db_path = DEFAULT_DB_PATH.to_string();
    }
    if cfg.max_body_bytes <= 0 {
        cfg.max_body_bytes = DEFAULT_MAX_BODY_BYTES;
    }
}

fn load_raw(path: &Path) -> Result<Config, ConfigError> {
    let data = fs::read_to_string(path)?;
    let mut cfg: Config = serde_yaml::from_str(&data)?;
    apply_defaults(&mut cfg);
    Ok(cfg)
}

/// Reads and validates a config that's ready to serve: it must have at
/// least one route (see [`validate`]).
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let cfg = load_raw(path)?;
    validate(&cfg)?;
    Ok(cfg)
}

/// Reads a config that may still be under construction from the GUI app:
/// an empty route table is fine (a brand-new install has none yet), and so
/// is a route with no keys — only [`validate_routes`]'s structural checks
/// apply, not [`validate`]'s "ready to serve" ones. If `path` doesn't exist
/// yet, writes out a blank config (no routes) via [`save`] and returns it,
/// so first launch has something to build on instead of erroring.
pub fn load_or_init(path: &Path) -> Result<Config, ConfigError> {
    if !path.exists() {
        let mut cfg = Config::default();
        apply_defaults(&mut cfg);
        save(path, &cfg)?;
        return Ok(cfg);
    }
    let cfg = load_raw(path)?;
    validate_routes(&cfg)?;
    Ok(cfg)
}

/// Serializes `cfg` as YAML and writes it to `path`, overwriting whatever
/// was there. Used by the GUI's save paths, which rewrite the whole file
/// rather than patching it in place — any hand-written comments/ordering
/// in an existing file are lost once the GUI edits it.
pub fn save(path: &Path, cfg: &Config) -> Result<(), ConfigError> {
    let data = serde_yaml::to_string(cfg)?;
    fs::write(path, data)?;
    Ok(())
}

/// Checks each route's own structure: required fields, a known format,
/// uniquely-labeled real keys, every virtual key resolving to one of
/// them, and no virtual key claimed by more than one route. Deliberately
/// allows a route to have zero real_keys/keys (see [`validate`] for the
/// check that a route is actually ready to serve).
pub fn validate_routes(cfg: &Config) -> Result<(), ConfigError> {
    if !cfg.shutdown_timeout.is_empty() {
        humantime::parse_duration(&cfg.shutdown_timeout).map_err(|e| {
            invalid(format!(
                "shutdown_timeout: invalid duration {:?}: {e}",
                cfg.shutdown_timeout
            ))
        })?;
    }

    let mut key_owner: HashMap<&str, &str> = HashMap::new();
    for r in &cfg.routes {
        if r.name.is_empty() {
            return Err(invalid("route: name is required"));
        }
        if r.format != "openai" && r.format != "anthropic" {
            return Err(invalid(format!(
                "route {:?}: format must be \"openai\" or \"anthropic\", got {:?}",
                r.name, r.format
            )));
        }
        if r.upstream.is_empty() {
            return Err(invalid(format!("route {:?}: upstream is required", r.name)));
        }

        let mut real_key_labels: HashMap<&str, ()> = HashMap::new();
        for rk in &r.real_keys {
            if rk.label.is_empty() {
                return Err(invalid(format!(
                    "route {:?}: every real_keys entry needs a label",
                    r.name
                )));
            }
            if rk.value.is_empty() {
                return Err(invalid(format!(
                    "route {:?}: real_keys {:?}: value is required",
                    r.name, rk.label
                )));
            }
            if real_key_labels.insert(&rk.label, ()).is_some() {
                return Err(invalid(format!(
                    "route {:?}: real_keys label {:?} is used more than once",
                    r.name, rk.label
                )));
            }
        }

        for k in &r.keys {
            if let Some(owner) = key_owner.insert(&k.value, &r.name) {
                return Err(invalid(format!(
                    "route {:?}: key already claimed by route {:?}",
                    r.name, owner
                )));
            }
            if !real_key_labels.contains_key(k.real_key.as_str()) {
                return Err(invalid(format!(
                    "route {:?}: key (label {:?}) has real_key {:?}, which is not one of this route's real_keys labels",
                    r.name, k.label, k.real_key
                )));
            }
        }
    }

    Ok(())
}

/// Checks that `cfg` is ready to serve: at least one route, every route
/// has at least one real key and one virtual key, plus everything
/// [`validate_routes`] checks.
pub fn validate(cfg: &Config) -> Result<(), ConfigError> {
    if cfg.routes.is_empty() {
        return Err(invalid("config has no routes"));
    }
    validate_routes(cfg)?;
    for r in &cfg.routes {
        if r.real_keys.is_empty() {
            return Err(invalid(format!(
                "route {:?}: real_keys must have at least one entry (the upstream provider credential(s) virtual keys resolve to)",
                r.name
            )));
        }
        if r.keys.is_empty() {
            return Err(invalid(format!(
                "route {:?}: keys must have at least one entry (use value: \"\" to match requests with no Authorization/x-api-key header)",
                r.name
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(name: &str) -> Route {
        Route {
            name: name.to_string(),
            format: "openai".to_string(),
            upstream: "https://api.openai.com".to_string(),
            real_keys: vec![RealKeyEntry {
                label: "main".to_string(),
                value: "sk-real".to_string(),
            }],
            keys: vec![KeyEntry {
                value: "vk-1".to_string(),
                label: String::new(),
                real_key: "main".to_string(),
            }],
        }
    }

    #[test]
    fn validate_accepts_well_formed_config() {
        let cfg = Config {
            routes: vec![route("r1")],
            ..Default::default()
        };
        assert!(validate(&cfg).is_ok());
    }

    #[test]
    fn validate_rejects_empty_routes() {
        let cfg = Config::default();
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn validate_routes_allows_empty_routes() {
        let cfg = Config::default();
        assert!(validate_routes(&cfg).is_ok());
    }

    #[test]
    fn validate_rejects_bad_format() {
        let mut r = route("r1");
        r.format = "bogus".to_string();
        let cfg = Config {
            routes: vec![r],
            ..Default::default()
        };
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn validate_rejects_duplicate_real_key_label() {
        let mut r = route("r1");
        r.real_keys.push(RealKeyEntry {
            label: "main".to_string(),
            value: "sk-other".to_string(),
        });
        let cfg = Config {
            routes: vec![r],
            ..Default::default()
        };
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn validate_rejects_virtual_key_claimed_by_two_routes() {
        let mut r1 = route("r1");
        let mut r2 = route("r2");
        r2.keys[0].value = r1.keys[0].value.clone();
        r1.keys[0].value = "shared".to_string();
        r2.keys[0].value = "shared".to_string();
        let cfg = Config {
            routes: vec![r1, r2],
            ..Default::default()
        };
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn validate_rejects_key_with_unknown_real_key() {
        let mut r = route("r1");
        r.keys[0].real_key = "nope".to_string();
        let cfg = Config {
            routes: vec![r],
            ..Default::default()
        };
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn validate_rejects_route_with_no_real_keys() {
        let mut r = route("r1");
        r.real_keys.clear();
        let cfg = Config {
            routes: vec![r],
            ..Default::default()
        };
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn validate_rejects_route_with_no_keys() {
        let mut r = route("r1");
        r.keys.clear();
        let cfg = Config {
            routes: vec![r],
            ..Default::default()
        };
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn shutdown_timeout_empty_is_allowed() {
        let cfg = Config::default();
        assert!(validate_routes(&cfg).is_ok());
    }

    #[test]
    fn shutdown_timeout_well_formed_is_accepted() {
        let cfg = Config {
            shutdown_timeout: "30s".to_string(),
            ..Default::default()
        };
        assert!(validate_routes(&cfg).is_ok());
    }

    #[test]
    fn shutdown_timeout_malformed_is_rejected() {
        let cfg = Config {
            shutdown_timeout: "not-a-duration".to_string(),
            ..Default::default()
        };
        assert!(validate_routes(&cfg).is_err());
    }

    #[test]
    fn apply_defaults_fills_in_unset_fields() {
        let mut cfg = Config::default();
        apply_defaults(&mut cfg);
        assert_eq!(cfg.listen, DEFAULT_LISTEN);
        assert_eq!(cfg.db_path, DEFAULT_DB_PATH);
        assert_eq!(cfg.max_body_bytes, DEFAULT_MAX_BODY_BYTES);
    }

    #[test]
    fn default_config_path_ends_with_app_id_and_filename() {
        let path = default_config_path();
        assert_eq!(path.file_name().unwrap(), "config.yaml");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), APP_ID);
    }

    #[test]
    fn load_parses_yaml_and_applies_defaults() {
        let dir = std::env::temp_dir().join(format!("tollgate-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        std::fs::write(
            &path,
            r#"
routes:
  - name: r1
    format: openai
    upstream: https://api.openai.com
    real_keys:
      - label: main
        value: sk-real
    keys:
      - value: vk-1
        real_key: main
pricing:
  gpt-4o:
    input_per_mtok: 2.5
    output_per_mtok: 10.0
"#,
        )
        .unwrap();

        let cfg = load(&path).unwrap();
        assert_eq!(cfg.listen, DEFAULT_LISTEN);
        assert_eq!(cfg.routes.len(), 1);
        assert_eq!(cfg.pricing["gpt-4o"].input_per_mtok, 2.5);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn temp_config_path(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("tollgate-config-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        (dir, path)
    }

    #[test]
    fn load_or_init_creates_default_when_missing() {
        let (dir, path) = temp_config_path("load-or-init-missing");
        assert!(!path.exists());

        let cfg = load_or_init(&path).unwrap();
        assert_eq!(cfg.listen, DEFAULT_LISTEN);
        assert!(cfg.routes.is_empty());
        assert!(path.exists());

        // Second call reads back what was just written, unchanged.
        let reread = load_or_init(&path).unwrap();
        assert_eq!(reread.listen, cfg.listen);
        assert!(reread.routes.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_or_init_allows_route_with_no_keys_yet() {
        let (dir, path) = temp_config_path("load-or-init-empty-route");
        let mut r = route("r1");
        r.real_keys.clear();
        r.keys.clear();
        let cfg = Config {
            routes: vec![r],
            ..Default::default()
        };
        save(&path, &cfg).unwrap();

        let loaded = load_or_init(&path).unwrap();
        assert_eq!(loaded.routes.len(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_or_init_rejects_structurally_invalid_config() {
        let (dir, path) = temp_config_path("load-or-init-invalid");
        let mut r = route("r1");
        r.format = "bogus".to_string();
        let cfg = Config {
            routes: vec![r],
            ..Default::default()
        };
        save(&path, &cfg).unwrap();

        assert!(load_or_init(&path).is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_then_load_round_trips() {
        let (dir, path) = temp_config_path("save-round-trip");
        let cfg = Config {
            routes: vec![route("r1")],
            pricing: HashMap::from([(
                "gpt-4o".to_string(),
                ModelPrice {
                    input_per_mtok: 2.5,
                    output_per_mtok: 10.0,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        save(&path, &cfg).unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.routes.len(), 1);
        assert_eq!(loaded.pricing["gpt-4o"].output_per_mtok, 10.0);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
