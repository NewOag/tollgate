//! `tollgate routes|keys|realkeys|pricing add|list|remove` — AI- and
//! human-friendly CLI for scripting config.yaml edits, instead of
//! hand-editing YAML or driving it through the desktop app.
//!
//! Reused as-is from the standalone `tollgate` server binary — an
//! already-running server picks up the change via the existing SIGHUP
//! reload mechanism; the Tauri app does not watch the file, so it needs
//! a restart instead.

use std::io::Write;
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use clap::{Args, Subcommand};
use rand::Rng;
use tollgate::config::{self, Config, KeyEntry, ModelPrice, RealKeyEntry, Route};
use tollgate::store::Store;

#[derive(Subcommand)]
pub enum Command {
    /// Manage routes (upstream + format).
    #[command(subcommand)]
    Routes(RoutesCommand),
    /// Manage virtual keys clients authenticate with.
    #[command(subcommand)]
    Keys(KeysCommand),
    /// Manage real (upstream provider) keys.
    #[command(subcommand)]
    RealKeys(RealKeysCommand),
    /// Manage per-model pricing.
    #[command(subcommand)]
    Pricing(PricingCommand),
    /// Delete all recorded requests from the local database.
    ResetDb,
}

#[derive(Subcommand)]
pub enum RoutesCommand {
    Add(RoutesAddArgs),
    List,
    Remove(RoutesRemoveArgs),
}

#[derive(Args)]
pub struct RoutesAddArgs {
    #[arg(long)]
    name: String,
    /// "openai", "anthropic", or "openai_responses".
    #[arg(long)]
    format: String,
    #[arg(long)]
    upstream: String,
}

#[derive(Args)]
pub struct RoutesRemoveArgs {
    #[arg(long)]
    name: String,
}

#[derive(Subcommand)]
pub enum KeysCommand {
    Add(KeysAddArgs),
    List(KeysListArgs),
    Remove(KeysRemoveArgs),
}

#[derive(Args)]
pub struct KeysAddArgs {
    #[arg(long)]
    route: String,
    /// Label of the route's real key (registered via `tollgate real-keys add`) this virtual key maps to.
    #[arg(long)]
    real_key: String,
    /// Omit to auto-generate a local placeholder virtual key.
    #[arg(long)]
    value: Option<String>,
    #[arg(long)]
    label: Option<String>,
}

#[derive(Args)]
pub struct KeysListArgs {
    #[arg(long)]
    route: Option<String>,
    #[arg(long)]
    show_full: bool,
}

#[derive(Args)]
pub struct KeysRemoveArgs {
    #[arg(long)]
    route: String,
    #[arg(long)]
    value: Option<String>,
    #[arg(long)]
    label: Option<String>,
}

#[derive(Subcommand)]
pub enum RealKeysCommand {
    Add(RealKeysAddArgs),
    List(RealKeysListArgs),
    Remove(RealKeysRemoveArgs),
}

#[derive(Args)]
pub struct RealKeysAddArgs {
    #[arg(long)]
    route: String,
    #[arg(long)]
    label: String,
    #[arg(long)]
    value: String,
}

#[derive(Args)]
pub struct RealKeysListArgs {
    #[arg(long)]
    route: Option<String>,
    #[arg(long)]
    show_full: bool,
}

#[derive(Args)]
pub struct RealKeysRemoveArgs {
    #[arg(long)]
    route: String,
    #[arg(long)]
    label: String,
}

#[derive(Subcommand)]
pub enum PricingCommand {
    Add(PricingAddArgs),
    List,
    Remove(PricingRemoveArgs),
}

#[derive(Args)]
pub struct PricingAddArgs {
    #[arg(long)]
    model: String,
    #[arg(long)]
    input_per_mtok: f64,
    #[arg(long)]
    output_per_mtok: f64,
    #[arg(long, default_value_t = 0.0)]
    cached_input_per_mtok: f64,
    #[arg(long, default_value_t = 0.0)]
    cache_write_per_mtok: f64,
}

#[derive(Args)]
pub struct PricingRemoveArgs {
    #[arg(long)]
    model: String,
}

/// Entry point called from `main.rs` when a subcommand was given: loads
/// (or initializes) the config at `config_path`, applies the command,
/// and prints a confirmation/next-step hint to stdout.
pub fn run(command: Command, config_path: &Path) -> Result<()> {
    run_to(command, config_path, &mut std::io::stdout())
}

fn run_to(command: Command, config_path: &Path, out: &mut impl Write) -> Result<()> {
    let mut cfg = config::load_or_init(config_path)?;
    if let Command::ResetDb = command {
        return reset_db(&cfg, out);
    }
    dispatch(command, &mut cfg, config_path, out)
}

fn dispatch(command: Command, cfg: &mut Config, config_path: &Path, out: &mut impl Write) -> Result<()> {
    match command {
        Command::Routes(RoutesCommand::Add(args)) => routes_add(cfg, args, config_path, out),
        Command::Routes(RoutesCommand::List) => routes_list(cfg, out),
        Command::Routes(RoutesCommand::Remove(args)) => routes_remove(cfg, args, config_path, out),
        Command::Keys(KeysCommand::Add(args)) => keys_add(cfg, args, config_path, out),
        Command::Keys(KeysCommand::List(args)) => keys_list(cfg, args, out),
        Command::Keys(KeysCommand::Remove(args)) => keys_remove(cfg, args, config_path, out),
        Command::RealKeys(RealKeysCommand::Add(args)) => real_keys_add(cfg, args, config_path, out),
        Command::RealKeys(RealKeysCommand::List(args)) => real_keys_list(cfg, args, out),
        Command::RealKeys(RealKeysCommand::Remove(args)) => real_keys_remove(cfg, args, config_path, out),
        Command::Pricing(PricingCommand::Add(args)) => pricing_add(cfg, args, config_path, out),
        Command::Pricing(PricingCommand::List) => pricing_list(cfg, out),
        Command::Pricing(PricingCommand::Remove(args)) => pricing_remove(cfg, args, config_path, out),
        Command::ResetDb => unreachable!("handled in run_to"),
    }
}

fn reset_db(cfg: &Config, out: &mut impl Write) -> Result<()> {
    let db_path = if cfg.db_path.is_empty() { config::DEFAULT_DB_PATH } else { &cfg.db_path };
    let store = Store::open(db_path)?;
    store.clear()?;
    writeln!(out, "reset-db: all request records deleted from {db_path:?}")?;
    Ok(())
}

fn routes_add(cfg: &mut Config, args: RoutesAddArgs, config_path: &Path, out: &mut impl Write) -> Result<()> {
    let RoutesAddArgs { name, format, upstream } = args;
    if cfg.routes.iter().any(|r| r.name == name) {
        bail!("routes add: route {name:?} already exists");
    }
    cfg.routes.push(Route {
        name: name.clone(),
        format,
        upstream,
        real_keys: Vec::new(),
        keys: Vec::new(),
    });
    config::validate_routes(cfg)?;
    config::save(config_path, cfg)?;
    writeln!(out, "added route {name:?}")?;
    writeln!(
        out,
        "next: tollgate --config {} real-keys add --route {name} --label <label> --value <real-provider-key>",
        config_path.display()
    )?;
    writeln!(
        out,
        "then: tollgate --config {} keys add --route {name} --real-key <label>",
        config_path.display()
    )?;
    Ok(())
}

fn routes_list(cfg: &Config, out: &mut impl Write) -> Result<()> {
    if cfg.routes.is_empty() {
        writeln!(out, "no routes configured yet; add one with `tollgate routes add`")?;
        return Ok(());
    }
    for r in &cfg.routes {
        writeln!(
            out,
            "{} format={} upstream={} real_keys={} keys={}",
            r.name,
            r.format,
            r.upstream,
            r.real_keys.len(),
            r.keys.len()
        )?;
    }
    Ok(())
}

fn routes_remove(cfg: &mut Config, args: RoutesRemoveArgs, config_path: &Path, out: &mut impl Write) -> Result<()> {
    let RoutesRemoveArgs { name } = args;
    let idx = cfg
        .routes
        .iter()
        .position(|r| r.name == name)
        .ok_or_else(|| anyhow!("routes remove: route {name:?} not found"))?;
    cfg.routes.remove(idx);
    config::save(config_path, cfg)?;
    writeln!(out, "removed route {name:?}")?;
    Ok(())
}

fn keys_add(cfg: &mut Config, args: KeysAddArgs, config_path: &Path, out: &mut impl Write) -> Result<()> {
    let KeysAddArgs { route, real_key, value, label } = args;
    let r = cfg
        .routes
        .iter_mut()
        .find(|r| r.name == route)
        .ok_or_else(|| anyhow!("keys add: route {route:?} not found"))?;
    if !r.real_keys.iter().any(|rk| rk.label == real_key) {
        bail!("keys add: route {route:?} has no real_keys labeled {real_key:?} (register it first with `tollgate real-keys add`)");
    }
    let value = value.unwrap_or_else(generate_virtual_key);
    let label = label.unwrap_or_default();
    r.keys.push(KeyEntry {
        value: value.clone(),
        label: label.clone(),
        real_key: real_key.clone(),
    });
    config::validate_routes(cfg)?;
    config::save(config_path, cfg)?;
    writeln!(out, "added key to route {route:?} (real_key {real_key:?}): value={value:?} label={label:?}")?;
    Ok(())
}

fn keys_list(cfg: &Config, args: KeysListArgs, out: &mut impl Write) -> Result<()> {
    let KeysListArgs { route, show_full } = args;
    let mut any = false;
    for r in &cfg.routes {
        if let Some(want) = &route
            && &r.name != want
        {
            continue;
        }
        for k in &r.keys {
            any = true;
            let value = if show_full { k.value.clone() } else { mask_key(&k.value) };
            let label = if k.label.is_empty() { r.name.clone() } else { k.label.clone() };
            writeln!(out, "{} label={label} value={value} real_key={}", r.name, k.real_key)?;
        }
    }
    if !any {
        writeln!(out, "no keys configured yet; add one with `tollgate keys add`")?;
    }
    Ok(())
}

fn keys_remove(cfg: &mut Config, args: KeysRemoveArgs, config_path: &Path, out: &mut impl Write) -> Result<()> {
    let KeysRemoveArgs { route, value, label } = args;
    let identifier = value
        .clone()
        .or_else(|| label.clone())
        .ok_or_else(|| anyhow!("keys remove: one of --value or --label is required"))?;
    let r = cfg
        .routes
        .iter_mut()
        .find(|r| r.name == route)
        .ok_or_else(|| anyhow!("keys remove: route {route:?} not found"))?;
    if r.keys.len() <= 1 {
        bail!(
            "keys remove: {identifier:?} is the only key on route {route:?}; use `tollgate routes remove --name {route}` to remove the whole route instead"
        );
    }
    let idx = r
        .keys
        .iter()
        .position(|k| value.as_deref().is_some_and(|v| k.value == v) || label.as_deref().is_some_and(|l| k.label == l))
        .ok_or_else(|| anyhow!("keys remove: route {route:?} has no key matching {identifier:?}"))?;
    r.keys.remove(idx);
    config::save(config_path, cfg)?;
    writeln!(out, "removed key {identifier:?} from route {route:?}")?;
    Ok(())
}

fn real_keys_add(cfg: &mut Config, args: RealKeysAddArgs, config_path: &Path, out: &mut impl Write) -> Result<()> {
    let RealKeysAddArgs { route, label, value } = args;
    let r = cfg
        .routes
        .iter_mut()
        .find(|r| r.name == route)
        .ok_or_else(|| anyhow!("real-keys add: route {route:?} not found"))?;
    r.real_keys.push(RealKeyEntry { label: label.clone(), value });
    config::validate_routes(cfg)?;
    config::save(config_path, cfg)?;
    writeln!(
        out,
        "added real key {label:?} to route {route:?}; next: tollgate --config {} keys add --route {route} --real-key {label}",
        config_path.display()
    )?;
    Ok(())
}

fn real_keys_list(cfg: &Config, args: RealKeysListArgs, out: &mut impl Write) -> Result<()> {
    let RealKeysListArgs { route, show_full } = args;
    let mut any = false;
    for r in &cfg.routes {
        if let Some(want) = &route
            && &r.name != want
        {
            continue;
        }
        for rk in &r.real_keys {
            any = true;
            let value = if show_full { rk.value.clone() } else { mask_key(&rk.value) };
            writeln!(out, "{} label={} value={value}", r.name, rk.label)?;
        }
    }
    if !any {
        writeln!(out, "no real keys configured yet; add one with `tollgate real-keys add`")?;
    }
    Ok(())
}

fn real_keys_remove(cfg: &mut Config, args: RealKeysRemoveArgs, config_path: &Path, out: &mut impl Write) -> Result<()> {
    let RealKeysRemoveArgs { route, label } = args;
    let r = cfg
        .routes
        .iter_mut()
        .find(|r| r.name == route)
        .ok_or_else(|| anyhow!("real-keys remove: route {route:?} not found"))?;
    if let Some(k) = r.keys.iter().find(|k| k.real_key == label) {
        let vk_label = k.label.clone();
        let vk_value = k.value.clone();
        bail!(
            "real-keys remove: virtual key {vk_label:?} (value {vk_value:?}) still maps to real key {label:?}; reassign or remove it first with `tollgate keys remove`"
        );
    }
    let idx = r
        .real_keys
        .iter()
        .position(|rk| rk.label == label)
        .ok_or_else(|| anyhow!("real-keys remove: route {route:?} has no real_keys labeled {label:?}"))?;
    r.real_keys.remove(idx);
    config::save(config_path, cfg)?;
    writeln!(out, "removed real key {label:?} from route {route:?}")?;
    Ok(())
}

fn pricing_add(cfg: &mut Config, args: PricingAddArgs, config_path: &Path, out: &mut impl Write) -> Result<()> {
    let PricingAddArgs {
        model,
        input_per_mtok,
        output_per_mtok,
        cached_input_per_mtok,
        cache_write_per_mtok,
    } = args;
    cfg.pricing.insert(
        model.clone(),
        ModelPrice {
            input_per_mtok,
            output_per_mtok,
            cached_input_per_mtok,
            cache_write_per_mtok,
        },
    );
    config::save(config_path, cfg)?;
    writeln!(out, "added pricing for {model:?}: input={input_per_mtok} output={output_per_mtok}")?;
    Ok(())
}

fn pricing_list(cfg: &Config, out: &mut impl Write) -> Result<()> {
    if cfg.pricing.is_empty() {
        writeln!(out, "no pricing configured yet; add one with `tollgate pricing add`")?;
        return Ok(());
    }
    let mut models: Vec<&String> = cfg.pricing.keys().collect();
    models.sort();
    for model in models {
        let p = &cfg.pricing[model];
        writeln!(
            out,
            "{model} input={} output={} cached_input={} cache_write={}",
            p.input_per_mtok, p.output_per_mtok, p.cached_input_per_mtok, p.cache_write_per_mtok
        )?;
    }
    Ok(())
}

fn pricing_remove(cfg: &mut Config, args: PricingRemoveArgs, config_path: &Path, out: &mut impl Write) -> Result<()> {
    let PricingRemoveArgs { model } = args;
    if cfg.pricing.remove(&model).is_none() {
        bail!("pricing remove: no pricing configured for model {model:?}");
    }
    config::save(config_path, cfg)?;
    writeln!(out, "removed pricing for {model:?}")?;
    Ok(())
}

/// Generates a placeholder virtual key value for `keys add` when
/// `--value` was omitted — good enough to unblock local testing;
/// production deployments should pass their own value.
fn generate_virtual_key() -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut rng = rand::thread_rng();
    let suffix: String = (0..24).map(|_| HEX[rng.gen_range(0..HEX.len())] as char).collect();
    format!("vk-{suffix}")
}

/// Masks a key value for display: empty stays a clear placeholder rather
/// than a blank field; short values are fully masked since showing any
/// character of a short secret narrows it down a lot; longer values show
/// a few characters at each edge so the right key can be recognized at a
/// glance without exposing the whole thing.
fn mask_key(value: &str) -> String {
    const EDGE: usize = 4;
    const MIN_LEN_TO_SHOW_EDGES: usize = EDGE * 2 + 4;

    if value.is_empty() {
        return "(empty)".to_string();
    }
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < MIN_LEN_TO_SHOW_EDGES {
        return "*".repeat(chars.len());
    }
    let prefix: String = chars[..EDGE].iter().collect();
    let suffix: String = chars[chars.len() - EDGE..].iter().collect();
    format!("{prefix}...{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_path(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("tollgate-cli-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        (dir, path)
    }

    fn add_route(path: &Path) {
        let mut out = Vec::new();
        run_to(
            Command::Routes(RoutesCommand::Add(RoutesAddArgs {
                name: "r1".to_string(),
                format: "openai".to_string(),
                upstream: "https://api.openai.com".to_string(),
            })),
            path,
            &mut out,
        )
        .unwrap();
    }

    fn add_real_key(path: &Path) {
        let mut out = Vec::new();
        run_to(
            Command::RealKeys(RealKeysCommand::Add(RealKeysAddArgs {
                route: "r1".to_string(),
                label: "main".to_string(),
                value: "sk-real".to_string(),
            })),
            path,
            &mut out,
        )
        .unwrap();
    }

    #[test]
    fn routes_add_list_remove_round_trip() {
        let (dir, path) = temp_config_path("routes-add-list-remove");
        add_route(&path);

        let mut out = Vec::new();
        run_to(Command::Routes(RoutesCommand::List), &path, &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("r1"));

        let mut out = Vec::new();
        run_to(
            Command::Routes(RoutesCommand::Remove(RoutesRemoveArgs { name: "r1".to_string() })),
            &path,
            &mut out,
        )
        .unwrap();

        let mut out = Vec::new();
        run_to(Command::Routes(RoutesCommand::List), &path, &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("no routes configured"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn routes_add_rejects_duplicate_name() {
        let (dir, path) = temp_config_path("routes-add-dup");
        add_route(&path);

        let mut out = Vec::new();
        let err = run_to(
            Command::Routes(RoutesCommand::Add(RoutesAddArgs {
                name: "r1".to_string(),
                format: "openai".to_string(),
                upstream: "https://api.openai.com".to_string(),
            })),
            &path,
            &mut out,
        )
        .unwrap_err();
        assert!(err.to_string().contains("already exists"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn real_keys_then_keys_add_round_trip() {
        let (dir, path) = temp_config_path("real-keys-then-keys-add");
        add_route(&path);
        add_real_key(&path);

        let mut out = Vec::new();
        run_to(
            Command::Keys(KeysCommand::Add(KeysAddArgs {
                route: "r1".to_string(),
                real_key: "main".to_string(),
                value: None,
                label: None,
            })),
            &path,
            &mut out,
        )
        .unwrap();
        assert!(String::from_utf8(out).unwrap().contains("added key"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keys_add_rejects_unknown_real_key() {
        let (dir, path) = temp_config_path("keys-add-unknown-real-key");
        add_route(&path);

        let mut out = Vec::new();
        let err = run_to(
            Command::Keys(KeysCommand::Add(KeysAddArgs {
                route: "r1".to_string(),
                real_key: "nope".to_string(),
                value: None,
                label: None,
            })),
            &path,
            &mut out,
        )
        .unwrap_err();
        assert!(err.to_string().contains("has no real_keys labeled"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keys_add_generates_value_when_omitted() {
        let (dir, path) = temp_config_path("keys-add-generate-value");
        add_route(&path);
        add_real_key(&path);

        let mut out = Vec::new();
        run_to(
            Command::Keys(KeysCommand::Add(KeysAddArgs {
                route: "r1".to_string(),
                real_key: "main".to_string(),
                value: None,
                label: None,
            })),
            &path,
            &mut out,
        )
        .unwrap();

        let cfg = config::load_or_init(&path).unwrap();
        assert!(!cfg.routes[0].keys[0].value.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keys_remove_refuses_to_remove_the_last_key() {
        let (dir, path) = temp_config_path("keys-remove-last-key");
        add_route(&path);
        add_real_key(&path);

        let mut out = Vec::new();
        run_to(
            Command::Keys(KeysCommand::Add(KeysAddArgs {
                route: "r1".to_string(),
                real_key: "main".to_string(),
                value: Some("vk-1".to_string()),
                label: None,
            })),
            &path,
            &mut out,
        )
        .unwrap();

        let mut out = Vec::new();
        let err = run_to(
            Command::Keys(KeysCommand::Remove(KeysRemoveArgs {
                route: "r1".to_string(),
                value: Some("vk-1".to_string()),
                label: None,
            })),
            &path,
            &mut out,
        )
        .unwrap_err();
        assert!(err.to_string().contains("is the only key on route"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn real_keys_remove_refuses_when_still_referenced() {
        let (dir, path) = temp_config_path("real-keys-remove-referenced");
        add_route(&path);
        add_real_key(&path);

        let mut out = Vec::new();
        run_to(
            Command::Keys(KeysCommand::Add(KeysAddArgs {
                route: "r1".to_string(),
                real_key: "main".to_string(),
                value: Some("vk-1".to_string()),
                label: None,
            })),
            &path,
            &mut out,
        )
        .unwrap();

        let mut out = Vec::new();
        let err = run_to(
            Command::RealKeys(RealKeysCommand::Remove(RealKeysRemoveArgs {
                route: "r1".to_string(),
                label: "main".to_string(),
            })),
            &path,
            &mut out,
        )
        .unwrap_err();
        assert!(err.to_string().contains("still maps to real key"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pricing_add_list_remove_round_trip() {
        let (dir, path) = temp_config_path("pricing-add-list-remove");

        let mut out = Vec::new();
        run_to(
            Command::Pricing(PricingCommand::Add(PricingAddArgs {
                model: "gpt-4o".to_string(),
                input_per_mtok: 2.5,
                output_per_mtok: 10.0,
                cached_input_per_mtok: 0.0,
                cache_write_per_mtok: 0.0,
            })),
            &path,
            &mut out,
        )
        .unwrap();

        let mut out = Vec::new();
        run_to(Command::Pricing(PricingCommand::List), &path, &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("gpt-4o"));

        let mut out = Vec::new();
        run_to(
            Command::Pricing(PricingCommand::Remove(PricingRemoveArgs { model: "gpt-4o".to_string() })),
            &path,
            &mut out,
        )
        .unwrap();

        let mut out = Vec::new();
        run_to(Command::Pricing(PricingCommand::List), &path, &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("no pricing configured"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pricing_remove_errors_on_unknown_model() {
        let (dir, path) = temp_config_path("pricing-remove-unknown");

        let mut out = Vec::new();
        let err = run_to(
            Command::Pricing(PricingCommand::Remove(PricingRemoveArgs { model: "nope".to_string() })),
            &path,
            &mut out,
        )
        .unwrap_err();
        assert!(err.to_string().contains("no pricing configured for model"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mask_key_labels_empty_value() {
        assert_eq!(mask_key(""), "(empty)");
    }

    #[test]
    fn mask_key_fully_masks_short_values() {
        assert_eq!(mask_key("sk-123"), "*".repeat(6));
    }

    #[test]
    fn mask_key_shows_prefix_and_suffix_for_long_values() {
        let masked = mask_key("sk-abcdefghijklmnop");
        assert!(masked.contains("..."));
        assert!(masked.starts_with("sk-a"));
        assert!(masked.ends_with("mnop"));
    }
}
