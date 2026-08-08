<script lang="ts">
  import { getStats, getTimeSeries } from "../api";
  import type { StatRow, TimeSeriesResult } from "../types";
  import { formatCost, formatTokens, formatLatency, formatPercent } from "../format";
  import { pushToast } from "../toast";
  import { defaultRange, previousEqualRange } from "../timeRange";
  import TimeRangePicker from "../components/TimeRangePicker.svelte";
  import StatCard from "../components/StatCard.svelte";
  import Card from "../components/Card.svelte";
  import LineChart from "../components/LineChart.svelte";
  import Table from "../components/Table.svelte";

  let range = $state(defaultRange());
  let rows = $state<StatRow[]>([]);
  let prevRows = $state<StatRow[]>([]);
  let costSeries = $state<TimeSeriesResult>({ buckets: [], series: {} });
  let countSeries = $state<TimeSeriesResult>({ buckets: [], series: {} });

  function sumRows(list: StatRow[]) {
    return list.reduce(
      (acc, r) => ({
        count: acc.count + r.count,
        total_tokens: acc.total_tokens + r.total_tokens,
        total_cost_usd: acc.total_cost_usd + r.total_cost_usd,
        errors: acc.errors + r.errors,
      }),
      { count: 0, total_tokens: 0, total_cost_usd: 0, errors: 0 },
    );
  }

  const totals = $derived(sumRows(rows));
  const prevTotals = $derived(sumRows(prevRows));

  const errorRate = $derived(totals.count > 0 ? totals.errors / totals.count : 0);
  const prevErrorRate = $derived(prevTotals.count > 0 ? prevTotals.errors / prevTotals.count : 0);

  function deltaPct(current: number, previous: number): number | null {
    if (previous <= 0) return null;
    return ((current - previous) / previous) * 100;
  }

  const sortedRows = $derived([...rows].sort((a, b) => b.total_cost_usd - a.total_cost_usd));

  $effect(() => {
    const r = range;
    let cancelled = false;
    (async () => {
      try {
        const prev = previousEqualRange(r);
        const [current, previous, cost, count] = await Promise.all([
          getStats(r.since, r.until, "model"),
          getStats(prev.since, prev.until, "model"),
          getTimeSeries(r.since, r.until, "model", "cost_usd"),
          getTimeSeries(r.since, r.until, "model", "count"),
        ]);
        if (cancelled) return;
        rows = current;
        prevRows = previous;
        costSeries = cost;
        countSeries = count;
      } catch (e) {
        if (!cancelled) pushToast(`Failed to load stats: ${e}`, "danger");
      }
    })();
    return () => {
      cancelled = true;
    };
  });
</script>

<div class="dashboard">
  <div class="toolbar">
    <h2>Dashboard</h2>
    <TimeRangePicker bind:value={range} />
  </div>

  <div class="stat-grid">
    <StatCard label="Requests" value={String(totals.count)} deltaPct={deltaPct(totals.count, prevTotals.count)} />
    <StatCard
      label="Total cost"
      value={formatCost(totals.total_cost_usd)}
      deltaPct={deltaPct(totals.total_cost_usd, prevTotals.total_cost_usd)}
    />
    <StatCard
      label="Total tokens"
      value={formatTokens(totals.total_tokens)}
      deltaPct={deltaPct(totals.total_tokens, prevTotals.total_tokens)}
    />
    <StatCard label="Error rate" value={formatPercent(errorRate)} deltaPct={deltaPct(errorRate, prevErrorRate)} />
  </div>

  <div class="chart-grid">
    <Card title="Cost over time">
      <LineChart buckets={costSeries.buckets} series={costSeries.series} />
    </Card>
    <Card title="Requests over time">
      <LineChart buckets={countSeries.buckets} series={countSeries.series} />
    </Card>
  </div>

  <Card title="By model">
    <Table>
      <thead>
        <tr>
          <th>Model</th>
          <th>Count</th>
          <th>Tokens</th>
          <th>Cache hit</th>
          <th>Avg latency</th>
          <th>Errors</th>
          <th>Cost</th>
        </tr>
      </thead>
      <tbody>
        {#each sortedRows as row (row.group)}
          <tr>
            <td>{row.group}</td>
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
  </Card>
</div>

<style>
  .dashboard {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
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

  .stat-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: var(--space-4);
  }

  .chart-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-4);
  }

  .empty {
    text-align: center;
    color: var(--text-muted);
    padding: var(--space-4);
  }
</style>
