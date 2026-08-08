<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { getConfig, getGatewayStatus, saveGeneralSettings } from "../api";
  import type { GatewayStatus } from "../types";
  import { pushToast } from "../toast";
  import Card from "../components/Card.svelte";
  import Button from "../components/Button.svelte";
  import Input from "../components/Input.svelte";
  import NumberInput from "../components/NumberInput.svelte";

  let status = $state<GatewayStatus | null>(null);
  let version = $state("");
  let maxBodyBytes = $state(0);
  let shutdownTimeout = $state("");
  let saving = $state(false);

  async function load() {
    try {
      const [cfg, gwStatus, v] = await Promise.all([getConfig(), getGatewayStatus(), getVersion()]);
      status = gwStatus;
      maxBodyBytes = cfg.max_body_bytes;
      shutdownTimeout = cfg.shutdown_timeout;
      version = v;
    } catch (e) {
      pushToast(`Failed to load settings: ${e}`, "danger");
    }
  }

  onMount(load);

  async function reveal(path: string) {
    try {
      await revealItemInDir(path);
    } catch (e) {
      pushToast(`Failed to reveal path: ${e}`, "danger");
    }
  }

  async function copy(value: string) {
    try {
      await navigator.clipboard.writeText(value);
      pushToast("Copied to clipboard");
    } catch {
      pushToast("Failed to copy", "danger");
    }
  }

  // A self-contained brief for handing to an AI assistant so it can drive
  // config CRUD through the `tollgate` CLI instead of hand-editing YAML.
  // Deliberately doesn't mention the config file's path: the CLI's
  // default --config already resolves to the same file this app uses (both
  // use the OS-standard per-app config directory), so naming it here would
  // only invite hand-editing it directly instead of going through the CLI.
  const aiPrompt = `You are managing a local LLM gateway called tollgate. Its config is a YAML file with routes (upstream + real/virtual API keys) and per-model pricing.

Do not hand-edit it — this desktop app rewrites the whole file on save, so hand-edits can be lost. Instead use the \`tollgate\` CLI, the same binary that runs the gateway: run it with no --config flag and it manages the same file this app uses.

Available commands:
  tollgate routes add --name <name> --format openai|anthropic --upstream <url>
  tollgate routes list
  tollgate routes remove --name <name>

  tollgate real-keys add --route <route> --label <label> --value <upstream-api-key>
  tollgate real-keys list [--route <route>] [--show-full]
  tollgate real-keys remove --route <route> --label <label>

  tollgate keys add --route <route> --real-key <real-key-label> [--value <v>] [--label <l>]
    (omit --value to auto-generate a local placeholder virtual key)
  tollgate keys list [--route <route>] [--show-full]
  tollgate keys remove --route <route> (--value <v> | --label <l>)

  tollgate pricing add --model <model> --input-per-mtok <f> --output-per-mtok <f> [--cached-input-per-mtok <f>] [--cache-write-per-mtok <f>]
  tollgate pricing list
  tollgate pricing remove --model <model>

Each command validates before writing, so a bad edit won't corrupt the file. After changing the config:
  - A standalone tollgate server process reloads automatically on SIGHUP.
  - This desktop app does NOT watch the file — it must be quit and reopened for changes to take effect.

Ask me what I want to add, change, or remove, then run the appropriate command(s).`;

  async function save() {
    saving = true;
    try {
      await saveGeneralSettings(maxBodyBytes, shutdownTimeout);
      pushToast("Settings saved");
    } catch (e) {
      pushToast(`Failed to save settings: ${e}`, "danger");
    } finally {
      saving = false;
    }
  }
</script>

<div class="settings-page">
  <div class="toolbar">
    <h2>Settings</h2>
  </div>

  <Card title="Storage">
    <div class="row">
      <div class="field">
        <span class="label">Config file</span>
        <span class="value num">{status?.config_path ?? ""}</span>
      </div>
      <Button variant="secondary" onclick={() => status && reveal(status.config_path)}>Show in Finder</Button>
    </div>
    <div class="row">
      <div class="field">
        <span class="label">Database file</span>
        <span class="value num">{status?.db_path ?? ""}</span>
      </div>
      <Button variant="secondary" onclick={() => status && reveal(status.db_path)}>Show in Finder</Button>
    </div>
    <p class="hint">
      Saving from this app rewrites the entire config file — any hand-written comments or key ordering will be lost.
    </p>
  </Card>

  <Card title="AI Assistant">
    <div class="ai-header">
      <p class="hint">
        Copy this brief and hand it to an AI assistant to manage routes, keys, and pricing through the <code>tollgate</code> CLI
        instead of editing config.yaml by hand.
      </p>
      <button class="link" onclick={() => copy(aiPrompt)}>Copy</button>
    </div>
    <pre class="ai-prompt">{aiPrompt}</pre>
  </Card>

  <Card title="Gateway">
    <div class="row">
      <div class="field">
        <span class="label">Listen address</span>
        <span class="value num">{status?.listen_addr ?? ""}</span>
      </div>
    </div>
    <p class="hint">Changing the listen address requires editing the config file by hand and restarting the app.</p>
    {#if status?.bind_error}
      <p class="error">Bind error: {status.bind_error}</p>
    {/if}
  </Card>

  <Card title="General">
    <div class="form-row">
      <label for="s-max-body">Max body bytes</label>
      <NumberInput bind:value={maxBodyBytes} min={0} step={1} />
    </div>
    <div class="form-row">
      <label for="s-shutdown">Shutdown timeout</label>
      <Input bind:value={shutdownTimeout} placeholder="5s" />
    </div>
    <div class="actions">
      <Button variant="primary" onclick={save} disabled={saving}>{saving ? "Saving…" : "Save"}</Button>
    </div>
  </Card>

  <Card title="About">
    <div class="row">
      <div class="field">
        <span class="label">Version</span>
        <span class="value num">{version}</span>
      </div>
    </div>
  </Card>
</div>

<style>
  .settings-page {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .toolbar h2 {
    font-size: 18px;
    font-weight: 600;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    padding: var(--space-2) 0;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .label {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .value {
    text-align: left;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hint {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: var(--space-2);
  }

  .hint code {
    font-family: var(--font-mono);
    color: var(--text);
  }

  .ai-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .ai-header .hint {
    margin-top: 0;
  }

  .ai-prompt {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    padding: var(--space-3);
    margin-top: var(--space-2);
    font-family: var(--font-mono);
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 280px;
    overflow-y: auto;
  }

  .error {
    font-size: 12px;
    color: var(--danger);
    margin-top: var(--space-2);
  }

  .form-row {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-bottom: var(--space-3);
  }

  .form-row label {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
  }

  .link {
    background: none;
    border: none;
    color: var(--accent);
    font-size: 12px;
    cursor: pointer;
    padding: 0;
    flex-shrink: 0;
  }

  .link:hover {
    color: var(--accent-hover);
  }
</style>
