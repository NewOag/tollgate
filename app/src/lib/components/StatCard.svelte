<script lang="ts">
  interface Props {
    label: string;
    value: string;
    deltaPct?: number | null;
  }

  let { label, value, deltaPct = null }: Props = $props();

  const trend = $derived(deltaPct === null || deltaPct === undefined ? null : deltaPct > 0 ? "up" : deltaPct < 0 ? "down" : "flat");
</script>

<div class="stat-card">
  <div class="label">{label}</div>
  <div class="value num">{value}</div>
  {#if trend}
    <div class="delta {trend}">
      <span class="arrow">{trend === "up" ? "↑" : trend === "down" ? "↓" : "→"}</span>
      <span class="num">{Math.abs(deltaPct ?? 0).toFixed(1)}%</span>
    </div>
  {/if}
</div>

<style>
  .stat-card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .label {
    font-size: 12px;
    color: var(--text-muted);
    font-weight: 500;
  }

  .value {
    font-size: 24px;
    font-weight: 600;
    color: var(--text);
    text-align: left;
  }

  .delta {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    font-weight: 500;
    width: fit-content;
  }

  .delta.up {
    color: var(--success);
  }

  .delta.down {
    color: var(--danger);
  }

  .delta.flat {
    color: var(--text-muted);
  }
</style>
