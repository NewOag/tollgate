<script lang="ts">
  import { onMount } from "svelte";
  import { getConfig, deleteRoute } from "../api";
  import type { Route } from "../types";
  import { pushToast } from "../toast";
  import { maskSecret, formatVariant } from "../format";
  import Card from "../components/Card.svelte";
  import Button from "../components/Button.svelte";
  import Badge from "../components/Badge.svelte";
  import RouteFormModal from "./RouteFormModal.svelte";

  let routes = $state<Route[]>([]);
  let expanded = $state<Set<string>>(new Set());
  let revealed = $state<Set<string>>(new Set());
  let showForm = $state(false);
  let editingRoute = $state<Route | null>(null);

  async function load() {
    try {
      const cfg = await getConfig();
      routes = cfg.routes;
    } catch (e) {
      pushToast(`Failed to load routes: ${e}`, "danger");
    }
  }

  onMount(load);

  function toggleExpand(name: string) {
    const next = new Set(expanded);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    expanded = next;
  }

  function toggleReveal(key: string) {
    const next = new Set(revealed);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    revealed = next;
  }

  async function copy(value: string) {
    try {
      await navigator.clipboard.writeText(value);
      pushToast("Copied to clipboard");
    } catch {
      pushToast("Failed to copy", "danger");
    }
  }

  function openCreate() {
    editingRoute = null;
    showForm = true;
  }

  function openEdit(route: Route) {
    editingRoute = route;
    showForm = true;
  }

  async function handleDelete(name: string) {
    if (!confirm(`Delete route "${name}"? This cannot be undone.`)) return;
    try {
      await deleteRoute(name);
      pushToast(`Deleted route "${name}"`);
      await load();
    } catch (e) {
      pushToast(`Failed to delete route: ${e}`, "danger");
    }
  }

  async function handleSaved() {
    showForm = false;
    await load();
  }
</script>

<div class="routes-page">
  <div class="toolbar">
    <h2>Routes & Keys</h2>
    <Button variant="primary" onclick={openCreate}>+ New route</Button>
  </div>

  {#each routes as route (route.name)}
    <Card>
      <div
        class="route-header"
        onclick={() => toggleExpand(route.name)}
        onkeydown={(e) => {
          if (e.key === "Enter" || e.key === " ") toggleExpand(route.name);
        }}
        role="button"
        tabindex="0"
      >
        <div class="route-title">
          <span class="chevron">{expanded.has(route.name) ? "▾" : "▸"}</span>
          <strong>{route.name}</strong>
          <Badge variant={formatVariant(route.format)}>{route.format}</Badge>
          <span class="upstream num">{route.upstream}</span>
        </div>
        <div class="route-actions">
          <Button
            variant="secondary"
            onclick={(e) => {
              e.stopPropagation();
              openEdit(route);
            }}>Edit</Button
          >
          <Button
            variant="danger"
            onclick={(e) => {
              e.stopPropagation();
              handleDelete(route.name);
            }}>Delete</Button
          >
        </div>
      </div>

      {#if expanded.has(route.name)}
        <div class="route-body">
          <div class="key-section">
            <h4>Real keys</h4>
            {#each route.real_keys as rk (rk.label)}
              {@const revealKey = `${route.name}:real:${rk.label}`}
              <div class="key-row">
                <span class="key-label">{rk.label}</span>
                <span class="key-value num">{revealed.has(revealKey) ? rk.value : maskSecret(rk.value)}</span>
                <button class="link" onclick={() => toggleReveal(revealKey)}>{revealed.has(revealKey) ? "Hide" : "Show"}</button>
                <button class="link" onclick={() => copy(rk.value)}>Copy</button>
              </div>
            {/each}
            {#if route.real_keys.length === 0}
              <p class="placeholder">No real keys.</p>
            {/if}
          </div>

          <div class="key-section">
            <h4>Virtual keys</h4>
            {#each route.keys as k (k.label)}
              {@const revealKey = `${route.name}:virtual:${k.label}`}
              <div class="key-row">
                <span class="key-label">{k.label}</span>
                <span class="key-target">→ {k.real_key}</span>
                <span class="key-value num">{revealed.has(revealKey) ? k.value : maskSecret(k.value)}</span>
                <button class="link" onclick={() => toggleReveal(revealKey)}>{revealed.has(revealKey) ? "Hide" : "Show"}</button>
                <button class="link" onclick={() => copy(k.value)}>Copy</button>
              </div>
            {/each}
            {#if route.keys.length === 0}
              <p class="placeholder">No virtual keys.</p>
            {/if}
          </div>
        </div>
      {/if}
    </Card>
  {/each}

  {#if routes.length === 0}
    <Card>
      <p class="placeholder">No routes configured yet. Create one to get started.</p>
    </Card>
  {/if}
</div>

{#if showForm}
  <RouteFormModal
    route={editingRoute}
    existingNames={routes.map((r) => r.name)}
    onclose={() => (showForm = false)}
    onsaved={handleSaved}
  />
{/if}

<style>
  .routes-page {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .toolbar h2 {
    font-size: 18px;
    font-weight: 600;
  }

  .route-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    cursor: pointer;
    gap: var(--space-4);
  }

  .route-title {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }

  .chevron {
    color: var(--text-muted);
    width: 12px;
  }

  .upstream {
    color: var(--text-muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .route-actions {
    display: flex;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .route-body {
    margin-top: var(--space-4);
    padding-top: var(--space-4);
    border-top: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .key-section h4 {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    margin-bottom: var(--space-2);
  }

  .key-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-1) 0;
    font-size: 13px;
  }

  .key-label {
    min-width: 120px;
    font-weight: 500;
  }

  .key-target {
    color: var(--text-muted);
    min-width: 100px;
  }

  .key-value {
    flex: 1;
    color: var(--text-muted);
  }

  .link {
    background: none;
    border: none;
    color: var(--accent);
    font-size: 12px;
    cursor: pointer;
    padding: 0;
  }

  .link:hover {
    color: var(--accent-hover);
  }

  .placeholder {
    color: var(--text-muted);
  }
</style>
