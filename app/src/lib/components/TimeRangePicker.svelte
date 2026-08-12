<script lang="ts">
  import type { TimeRange } from "../types";
  import { PRESETS, presetRangeForKey } from "../timeRange";

  interface Props {
    value: TimeRange;
    activeKey: string;
  }

  let { value = $bindable(), activeKey = $bindable() }: Props = $props();

  function toLocalInput(iso: string): string {
    const d = new Date(iso);
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }

  let customOpen = $state(activeKey === "custom");
  let customSince = $state(toLocalInput(value.since));
  let customUntil = $state(toLocalInput(value.until));

  function pickPreset(key: string) {
    activeKey = key;
    customOpen = false;
    value = presetRangeForKey(key);
  }

  function openCustom() {
    activeKey = "custom";
    customOpen = true;
    customSince = toLocalInput(value.since);
    customUntil = toLocalInput(value.until);
  }

  function applyCustom() {
    if (!customSince || !customUntil) return;
    value = { since: new Date(customSince).toISOString(), until: new Date(customUntil).toISOString() };
  }
</script>

<div class="time-range">
  <div class="presets">
    {#each PRESETS as p (p.key)}
      <button type="button" class="pill" class:active={activeKey === p.key} onclick={() => pickPreset(p.key)}>
        {p.label}
      </button>
    {/each}
    <button type="button" class="pill" class:active={activeKey === "custom"} onclick={openCustom}>Custom</button>
  </div>

  {#if customOpen}
    <div class="custom-range">
      <input type="datetime-local" bind:value={customSince} />
      <span class="sep">&ndash;</span>
      <input type="datetime-local" bind:value={customUntil} />
      <button type="button" class="apply" onclick={applyCustom}>Apply</button>
    </div>
  {/if}
</div>

<style>
  .time-range {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    align-items: flex-end;
  }

  .presets {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--space-1);
  }

  .pill {
    height: 28px;
    padding: 0 var(--space-3);
    border-radius: var(--radius-pill);
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    white-space: nowrap;
  }

  .pill:hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  .pill.active {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--bg);
  }

  .custom-range {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .custom-range input {
    height: 28px;
    padding: 0 var(--space-2);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    color: var(--text);
    font-size: 12px;
    font-family: var(--font-mono);
  }

  .sep {
    color: var(--text-muted);
  }

  .apply {
    height: 28px;
    padding: 0 var(--space-3);
    border-radius: var(--radius-control);
    border: none;
    background: var(--accent);
    color: var(--bg);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
  }

  .apply:hover {
    background: var(--accent-hover);
  }
</style>
