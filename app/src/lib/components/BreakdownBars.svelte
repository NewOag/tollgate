<script lang="ts">
  import { scaleLinear } from "d3-scale";
  import { CHART_COLORS } from "../chartColors";

  interface Item {
    label: string;
    value: number;
  }

  interface Props {
    items: Item[];
    formatValue?: (v: number) => string;
  }

  let { items, formatValue = (v: number) => String(v) }: Props = $props();

  const maxValue = $derived(Math.max(1, ...items.map((i) => i.value)));

  const widthScale = $derived(scaleLinear().domain([0, maxValue]).range([0, 100]));
</script>

<div class="bars">
  {#each items as item, i (item.label)}
    <div class="row">
      <span class="label">{item.label}</span>
      <div class="track">
        <div
          class="fill"
          style="width: {widthScale(item.value)}%; background: {CHART_COLORS[i % CHART_COLORS.length]};"
        ></div>
      </div>
      <span class="value num">{formatValue(item.value)}</span>
    </div>
  {/each}
  {#if items.length === 0}
    <p class="empty">No data</p>
  {/if}
</div>

<style>
  .bars {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .row {
    display: grid;
    grid-template-columns: 96px 1fr 72px;
    align-items: center;
    gap: var(--space-3);
  }

  .label {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track {
    height: 8px;
    background: var(--bg);
    border-radius: var(--radius-pill);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    border-radius: var(--radius-pill);
  }

  .value {
    font-size: 12px;
    color: var(--text);
  }

  .empty {
    font-size: 13px;
    color: var(--text-muted);
  }
</style>
