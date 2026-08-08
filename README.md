# tollgate

English | [中文](README.zh-CN.md)

A local LLM gateway: sits in front of your OpenAI/Anthropic-compatible client
SDKs, forwards requests to the real upstream unmodified, and logs
usage/latency/cost/full request-response content to a local SQLite file.

Ships as two things built from the same core crate:

- **`tollgate`** — a standalone CLI/server binary. Run it with no
  subcommand to start the gateway; use `routes`/`keys`/`real-keys`/`pricing`
  subcommands to script `config.yaml` edits.
- **Desktop app** (`app/`) — a Tauri + Svelte GUI that runs the same
  gateway in-process and gives you a dashboard, request browser, and
  routes/keys/pricing editor.

## Why

Client SDKs (OpenAI's, Anthropic's, anything that just does HTTP) only need
a base URL and an API key. Point them at `tollgate` instead of the real
provider, and you get, for free and entirely on your machine:

- Per-request logging: model, tokens, cost, latency, full request/response
  bodies and headers, streamed responses (SSE) captured byte-for-byte.
- Cost accounting from your own USD-per-million-token pricing table.
- Multiple upstream ("real") keys behind a single route, and multiple
  virtual keys per route for handing out scoped credentials without ever
  exposing the real one.
- Session grouping via an `x-session-id` header, for tying multi-turn
  conversations together in the UI.

## How routing works

Each incoming request is matched to a route purely by the API key it
presents (`Authorization: Bearer ...` or `x-api-key`) — not by path or
host. A route's `keys` list maps virtual keys to one of that route's
`real_keys`; the gateway swaps the virtual key for the real one before
forwarding, and everything else about the request (path, query, body,
other headers) passes through untouched.

## Quick start (CLI)

```sh
cargo build --release

# Register a route, its upstream credential, and a virtual key for it
./target/release/tollgate routes add --name openai --format openai --upstream https://api.openai.com
./target/release/tollgate real-keys add --route openai --label main --value sk-...
./target/release/tollgate keys add --route openai --real-key main --label my-app
# -> prints the generated virtual key; give that to your client SDK instead of the real one

./target/release/tollgate pricing add --model gpt-4o --input-per-mtok 2.5 --output-per-mtok 10

./target/release/tollgate   # starts the gateway, listening on :8787 by default
```

Point your client at `http://localhost:8787` with the virtual key from
`keys add`, using it exactly like the real provider's base URL.

Config lives at the OS-standard per-app config directory by default (e.g.
`~/Library/Application Support/com.tollgate.app/config.yaml` on macOS) so
the CLI and desktop app manage the same file out of the box. Override with
`--config <path>`.

### CLI reference

```
tollgate routes add --name <name> --format openai|anthropic --upstream <url>
tollgate routes list
tollgate routes remove --name <name>

tollgate real-keys add --route <route> --label <label> --value <upstream-api-key>
tollgate real-keys list [--route <route>] [--show-full]
tollgate real-keys remove --route <route> --label <label>

tollgate keys add --route <route> --real-key <real-key-label> [--value <v>] [--label <l>]
  (omit --value to auto-generate a virtual key)
tollgate keys list [--route <route>] [--show-full]
tollgate keys remove --route <route> (--value <v> | --label <l>)

tollgate pricing add --model <model> --input-per-mtok <f> --output-per-mtok <f> [--cached-input-per-mtok <f>] [--cache-write-per-mtok <f>]
tollgate pricing list
tollgate pricing remove --model <model>
```

Every command validates before writing, so a bad edit can't corrupt the
config file. A running standalone server reloads automatically on SIGHUP
after any change.

## Desktop app

```sh
cd app
npm install
npm run tauri dev    # dev build
npm run tauri build   # release bundle
```

The app runs the gateway in-process (no separate server to manage) and
does **not** watch the config file for external changes — restart it after
editing `config.yaml` by hand or via the CLI.

## Config reference

```yaml
listen: ":8787"          # default
db_path: "./tollgate.db" # default
max_body_bytes: 26214400 # 25 MiB default; larger request bodies are rejected (413)
shutdown_timeout: "30s"  # optional; how long graceful shutdown waits for in-flight requests

routes:
  - name: openai
    format: openai        # "openai" or "anthropic"
    upstream: https://api.openai.com
    real_keys:
      - label: main
        value: sk-...
    keys:
      - value: vk-...      # matched verbatim against the client's Authorization/x-api-key
        label: my-app
        real_key: main

pricing:
  gpt-4o:
    input_per_mtok: 2.5
    output_per_mtok: 10.0
    cached_input_per_mtok: 1.25   # optional, falls back to input_per_mtok
    cache_write_per_mtok: 3.75    # optional, falls back to input_per_mtok
```

## Releases

Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds the
desktop app for macOS (Apple Silicon) and Windows and publishes both as a
GitHub Release.

## Project layout

```
src/            core gateway/store/config/CLI library + `tollgate` binary
app/            Tauri + Svelte desktop app (runs the same core in-process)
app/src-tauri/  Tauri host (Rust)
app/src/        Svelte frontend
```
