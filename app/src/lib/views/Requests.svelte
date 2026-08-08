<script lang="ts">
  import { onMount } from "svelte";
  import {
    getConfig,
    listRequests,
    getRequest,
    listDistinctModels,
    listDistinctVirtualKeyLabels,
    listDistinctSessionIds,
  } from "../api";
  import type { Route, RequestSummary, RequestRecord } from "../types";
  import { formatCost, formatTokens, formatLatency, formatTimestamp, formatPercent, statusVariant, formatVariant } from "../format";
  import { pushToast } from "../toast";
  import { defaultRange } from "../timeRange";
  import { highlightJson } from "../jsonHighlight";
  import TimeRangePicker from "../components/TimeRangePicker.svelte";
  import Dropdown from "../components/Dropdown.svelte";
  import Button from "../components/Button.svelte";
  import Badge from "../components/Badge.svelte";
  import Card from "../components/Card.svelte";
  import Table from "../components/Table.svelte";

  const limit = 25;

  let range = $state(defaultRange());
  let routeFilter = $state("");
  let modelFilter = $state("");
  let vkFilter = $state("");
  let sessionFilter = $state("");
  let page = $state(0);

  let routes = $state<Route[]>([]);
  let models = $state<string[]>([]);
  let vks = $state<string[]>([]);
  let sessions = $state<string[]>([]);

  let rows = $state<RequestSummary[]>([]);
  let total = $state(0);

  let selectedId = $state<number | null>(null);
  let detail = $state<RequestRecord | null>(null);
  let detailLoading = $state(false);
  let rawView = $state(false);

  const DRAWER_WIDTH_KEY = "tollgate.requestDrawerWidth";
  const DRAWER_WIDTH_MIN = 320;
  const DRAWER_WIDTH_MAX = 900;
  const DRAWER_WIDTH_DEFAULT = 480;

  function loadDrawerWidth(): number {
    const raw = Number(localStorage.getItem(DRAWER_WIDTH_KEY));
    if (!Number.isFinite(raw) || raw <= 0) return DRAWER_WIDTH_DEFAULT;
    return Math.min(DRAWER_WIDTH_MAX, Math.max(DRAWER_WIDTH_MIN, raw));
  }

  let drawerWidth = $state(loadDrawerWidth());
  let dragging = $state(false);
  let dragStartX = 0;
  let dragStartWidth = 0;

  function startResize(e: MouseEvent) {
    e.preventDefault();
    dragging = true;
    dragStartX = e.clientX;
    dragStartWidth = drawerWidth;
  }

  function onDragMove(e: MouseEvent) {
    if (!dragging) return;
    const delta = dragStartX - e.clientX;
    drawerWidth = Math.min(DRAWER_WIDTH_MAX, Math.max(DRAWER_WIDTH_MIN, dragStartWidth + delta));
  }

  function stopResize() {
    if (!dragging) return;
    dragging = false;
    localStorage.setItem(DRAWER_WIDTH_KEY, String(drawerWidth));
  }

  onMount(async () => {
    try {
      const [cfg, modelList, vkList, sessionList] = await Promise.all([
        getConfig(),
        listDistinctModels(),
        listDistinctVirtualKeyLabels(),
        listDistinctSessionIds(),
      ]);
      routes = cfg.routes;
      models = modelList;
      vks = vkList;
      sessions = sessionList;
    } catch (e) {
      pushToast(`Failed to load filters: ${e}`, "danger");
    }
  });

  $effect(() => {
    void range;
    void routeFilter;
    void modelFilter;
    void vkFilter;
    void sessionFilter;
    page = 0;
  });

  $effect(() => {
    const args = {
      since: range.since,
      until: range.until,
      route: routeFilter || undefined,
      model: modelFilter || undefined,
      virtual_key_label: vkFilter || undefined,
      session_id: sessionFilter || undefined,
      limit,
      offset: page * limit,
    };
    let cancelled = false;
    (async () => {
      try {
        const [list, count] = await listRequests(args);
        if (cancelled) return;
        rows = list;
        total = count;
      } catch (e) {
        if (!cancelled) pushToast(`Failed to load requests: ${e}`, "danger");
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  async function openDetail(id: number) {
    selectedId = id;
    detail = null;
    detailLoading = true;
    try {
      detail = await getRequest(id);
    } catch (e) {
      pushToast(`Failed to load request ${id}: ${e}`, "danger");
    } finally {
      detailLoading = false;
    }
  }

  function closeDetail() {
    selectedId = null;
    detail = null;
    rawView = false;
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") closeDetail();
  }

  const pageCount = $derived(Math.max(1, Math.ceil(total / limit)));

  // The request record freezes the labels that were live at the moment
  // it was logged. If the virtual key was renamed since, resolve the
  // *current* label by matching the key's value (stored on the record)
  // against the currently loaded config — falling back to the frozen
  // historical labels when no match is found (renamed and value-rotated,
  // or an older record predating this lookup, which has an empty value).
  const currentKeyLabels = $derived.by(() => {
    if (!detail || !detail.virtual_key_value) return null;
    for (const r of routes) {
      const k = r.keys.find((k) => k.value === detail!.virtual_key_value);
      if (k) return { virtual: k.label || r.name, real: k.real_key };
    }
    return null;
  });

  const displayVirtualKeyLabel = $derived(currentKeyLabels?.virtual ?? detail?.virtual_key_label ?? "");
  const displayRealKeyLabel = $derived(currentKeyLabels?.real ?? detail?.real_key_label ?? "");

  const cacheHitRate = $derived(
    detail && detail.prompt_tokens > 0 ? detail.cache_read_tokens / detail.prompt_tokens : 0,
  );
</script>

<svelte:window onkeydown={onKeydown} onmousemove={onDragMove} onmouseup={stopResize} />

<div class="requests">
  <div class="toolbar">
    <h2>Requests</h2>
    <TimeRangePicker bind:value={range} />
  </div>

  <div class="filters">
    <div class="field">
      <label for="f-route">Route</label>
      <Dropdown
        bind:value={routeFilter}
        options={[{ value: "", label: "All routes" }, ...routes.map((r) => ({ value: r.name, label: r.name }))]}
      />
    </div>
    <div class="field">
      <label for="f-model">Model</label>
      <Dropdown
        bind:value={modelFilter}
        options={[{ value: "", label: "All models" }, ...models.map((m) => ({ value: m, label: m }))]}
      />
    </div>
    <div class="field">
      <label for="f-vk">Virtual key</label>
      <Dropdown bind:value={vkFilter} options={[{ value: "", label: "All keys" }, ...vks.map((v) => ({ value: v, label: v }))]} />
    </div>
    <div class="field">
      <label for="f-session">Session</label>
      <Dropdown
        bind:value={sessionFilter}
        options={[{ value: "", label: "All sessions" }, ...sessions.map((s) => ({ value: s, label: s }))]}
      />
    </div>
  </div>

  <Card>
    <Table>
      <thead>
        <tr>
          <th>Time</th>
          <th>Route</th>
          <th>Model</th>
          <th>Status</th>
          <th>Latency</th>
          <th>Tokens</th>
          <th>Cost</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as r (r.id)}
          <tr class="clickable" onclick={() => openDetail(r.id)}>
            <td class="num">{formatTimestamp(r.timestamp)}</td>
            <td>{r.route}</td>
            <td>{r.model}</td>
            <td><Badge variant={statusVariant(r.status_code)}>{r.status_code}</Badge></td>
            <td class="num">{formatLatency(r.latency_ms)}</td>
            <td class="num">{formatTokens(r.total_tokens)}</td>
            <td class="num">{formatCost(r.cost_usd)}</td>
          </tr>
        {/each}
        {#if rows.length === 0}
          <tr><td colspan="7" class="empty">No requests match these filters</td></tr>
        {/if}
      </tbody>
    </Table>

    <div class="pagination">
      <Button variant="ghost" disabled={page === 0} onclick={() => (page -= 1)}>Prev</Button>
      <span class="page-info num">Page {page + 1} of {pageCount}</span>
      <Button variant="ghost" disabled={(page + 1) * limit >= total} onclick={() => (page += 1)}>Next</Button>
    </div>
  </Card>
</div>

{#if selectedId !== null}
  <div class="drawer-backdrop" onclick={closeDetail} role="presentation"></div>
  <div class="drawer" role="dialog" aria-modal="true" style="width: {drawerWidth}px">
    <div class="drawer-resize-handle" aria-hidden="true" onmousedown={startResize}></div>
    <div class="drawer-header">
      <h3>Request #{selectedId}</h3>
      <div class="header-actions">
        {#if detail}
          <button class="link" onclick={() => (rawView = !rawView)}>{rawView ? "Formatted" : "Raw HTTP"}</button>
        {/if}
        <button class="close" onclick={closeDetail} aria-label="Close">&times;</button>
      </div>
    </div>
    <div class="drawer-body">
      {#if detailLoading}
        <p class="placeholder">Loading…</p>
      {:else if detail}
        <div class="meta-section">
          <h4>Overview</h4>
          <div class="meta-grid">
            <span class="meta-label">Timestamp</span><span class="num">{formatTimestamp(detail.timestamp)}</span>
            <span class="meta-label">Route</span><span>{detail.route}</span>
            <span class="meta-label">Format</span><span><Badge variant={formatVariant(detail.format)}>{detail.format}</Badge></span>
            <span class="meta-label">Model</span><span>{detail.model}</span>
            <span class="meta-label">Status</span><span><Badge variant={statusVariant(detail.status_code)}>{detail.status_code}</Badge></span>
          </div>
        </div>

        <div class="meta-section">
          <h4>Performance</h4>
          <div class="meta-grid">
            <span class="meta-label">Latency</span><span class="num">{formatLatency(detail.latency_ms)}</span>
            <span class="meta-label">Stream</span><span>{detail.stream ? "yes" : "no"}</span>
          </div>
        </div>

        <div class="meta-section">
          <h4>Session &amp; Keys</h4>
          <div class="meta-grid">
            <span class="meta-label">Session</span><span>{detail.session_id || "—"}</span>
            <span class="meta-label">Virtual key</span><span>{displayVirtualKeyLabel}</span>
            <span class="meta-label">Real key</span><span>{displayRealKeyLabel}</span>
          </div>
        </div>

        <div class="meta-section">
          <h4>Tokens &amp; Cost</h4>
          <div class="meta-grid">
            <span class="meta-label">Prompt tokens</span><span class="num">{detail.prompt_tokens}</span>
            <span class="meta-label">Completion tokens</span><span class="num">{detail.completion_tokens}</span>
            <span class="meta-label">Cache creation</span><span class="num">{detail.cache_creation_tokens}</span>
            <span class="meta-label">Cache read</span><span class="num">{detail.cache_read_tokens}</span>
            <span class="meta-label">Cache hit rate</span><span class="num">{formatPercent(cacheHitRate)}</span>
            <span class="meta-label">Total tokens</span><span class="num">{detail.total_tokens}</span>
            <span class="meta-label">Cost</span><span class="num">{formatCost(detail.cost_usd)}</span>
          </div>
        </div>

        {#if detail.error}
          <div class="error-block">{detail.error}</div>
        {/if}

        <div class="body-block">
          <h4>Request {rawView ? "(raw HTTP)" : "body"}</h4>
          <pre>{@html highlightJson(rawView ? `${detail.request_headers}\n${detail.request_body}` : detail.request_body)}</pre>
        </div>
        <div class="body-block">
          <h4>Response {rawView ? "(raw HTTP)" : "body"}</h4>
          <pre>{@html highlightJson(rawView ? `${detail.response_headers}\n${detail.response_body}` : detail.response_body)}</pre>
        </div>
      {:else}
        <p class="placeholder">Not found.</p>
      {/if}
    </div>
  </div>
{/if}

<style>
  .requests {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .toolbar {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
  }

  .toolbar h2 {
    font-size: 18px;
    font-weight: 600;
  }

  .filters {
    display: flex;
    gap: var(--space-4);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    width: 180px;
  }

  .field label {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .clickable {
    cursor: pointer;
  }

  .empty {
    text-align: center;
    color: var(--text-muted);
    padding: var(--space-4);
  }

  .pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-4);
    padding-top: var(--space-4);
  }

  .page-info {
    color: var(--text-muted);
    font-size: 12px;
  }

  .drawer-backdrop {
    position: fixed;
    inset: 0;
    background: color-mix(in srgb, #000000 45%, transparent);
    z-index: 100;
  }

  .drawer {
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    max-width: calc(100vw - var(--space-8));
    background: var(--surface);
    border-left: 1px solid var(--border);
    box-shadow: -12px 0 32px color-mix(in srgb, #000000 40%, transparent);
    z-index: 101;
    display: flex;
    flex-direction: column;
  }

  .drawer-resize-handle {
    position: absolute;
    top: 0;
    left: 0;
    bottom: 0;
    width: 6px;
    cursor: col-resize;
    z-index: 1;
    user-select: none;
  }

  .drawer-resize-handle:hover {
    background: var(--accent);
    opacity: 0.3;
  }

  .drawer-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .drawer-header h3 {
    font-size: 15px;
    font-weight: 600;
    font-family: var(--font-mono);
  }

  .close {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 20px;
    line-height: 1;
    cursor: pointer;
    padding: 0;
  }

  .close:hover {
    color: var(--text);
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

  .drawer-body {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .placeholder {
    color: var(--text-muted);
  }

  .meta-section h4 {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    margin-bottom: var(--space-2);
  }

  .meta-grid {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-2) var(--space-3);
    font-size: 13px;
  }

  .meta-label {
    color: var(--text-muted);
  }

  .error-block {
    background: color-mix(in srgb, var(--danger) 15%, transparent);
    color: var(--danger);
    padding: var(--space-3);
    border-radius: var(--radius-control);
    font-size: 13px;
    white-space: pre-wrap;
  }

  .body-block h4 {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    margin-bottom: var(--space-2);
  }

  .body-block pre {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    padding: var(--space-3);
    font-family: var(--font-mono);
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 320px;
    overflow-y: auto;
  }

  .body-block pre :global(.jt-key) {
    color: var(--accent);
  }

  .body-block pre :global(.jt-string) {
    color: var(--success);
  }

  .body-block pre :global(.jt-number) {
    color: var(--warning);
  }

  .body-block pre :global(.jt-bool),
  .body-block pre :global(.jt-null) {
    color: var(--chart-3);
  }
</style>
