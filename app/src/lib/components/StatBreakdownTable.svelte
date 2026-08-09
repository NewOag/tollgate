<script lang="ts">
  import type { StatRow } from "../types";
  import { formatCost, formatTokens, formatLatency, formatPercent } from "../format";
  import Table from "./Table.svelte";

  interface Props {
    groupLabel: string;
    rows: StatRow[];
    resolveDisplayLabel?: (row: StatRow) => string;
  }

  let { groupLabel, rows, resolveDisplayLabel = (row) => row.group_label || row.group }: Props = $props();

  const sortedRows = $derived([...rows].sort((a, b) => b.total_cost_usd - a.total_cost_usd));
</script>

<Table>
  <thead>
    <tr>
      <th>{groupLabel}</th>
      <th class="num">Count</th>
      <th class="num">Tokens</th>
      <th class="num">Cache hit</th>
      <th class="num">Avg latency</th>
      <th class="num">Errors</th>
      <th class="num">Cost</th>
    </tr>
  </thead>
  <tbody>
    {#each sortedRows as row (row.group)}
      <tr>
        <td>{resolveDisplayLabel(row) || "—"}</td>
        <td class="num">{row.count}</td>
        <td class="num">{formatTokens(row.total_tokens)}</td>
        <td class="num">{formatPercent(row.cache_hit_rate)}</td>
        <td class="num">{formatLatency(row.avg_latency_ms)}</td>
        <td class="num">{row.errors}</td>
        <td class="num">{formatCost(row.total_cost_usd)}</td>
      </tr>
    {/each}
    {#if sortedRows.length === 0}
      <tr><td colspan="7" class="empty">No data for this window</td></tr>
    {/if}
  </tbody>
</Table>

<style>
  .empty {
    text-align: center;
    color: var(--text-muted);
    padding: var(--space-4);
  }
</style>
