<script lang="ts">
  import { onMount } from "svelte";
  import { getConfig, getStats, getTimeSeries } from "../api";
  import type { Route, StatRow, TimeSeriesResult } from "../types";
  import { formatCost, formatTokens, formatPercent } from "../format";
  import { pushToast } from "../toast";
  import { defaultRange, previousEqualRange } from "../timeRange";
  import TimeRangePicker from "../components/TimeRangePicker.svelte";
  import RefreshControl from "../components/RefreshControl.svelte";
  import StatCard from "../components/StatCard.svelte";
  import Card from "../components/Card.svelte";
  import LineChart from "../components/LineChart.svelte";
  import StatBreakdownTable from "../components/StatBreakdownTable.svelte";

  let range = $state(defaultRange());
  let rows = $state<StatRow[]>([]);
  let prevRows = $state<StatRow[]>([]);
  let vkRows = $state<StatRow[]>([]);
  let costSeries = $state<TimeSeriesResult>({ buckets: [], series: {} });
  let countSeries = $state<TimeSeriesResult>({ buckets: [], series: {} });
  let routes = $state<Route[]>([]);

  onMount(async () => {
    try {
      routes = (await getConfig()).routes;
    } catch (e) {
      pushToast(`Failed to load config: ${e}`, "danger");
    }
  });

  // `group` for virtual_key rows is the key's stable value, which may
  // have been renamed since the request was logged — resolve against the
  // current config first, falling back to the frozen historical label
  // (`group_label`) if the key was since deleted or config hasn't loaded.
  function resolveVirtualKeyLabel(row: StatRow): string {
    for (const r of routes) {
      const k = r.keys.find((k) => k.value === row.group);
      if (k) return k.label || r.name;
    }
    return row.group_label || row.group;
  }

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

  let loading = $state(false);
  let statsSeq = 0;

  async function loadStats() {
    const seq = ++statsSeq;
    const r = range;
    loading = true;
    try {
      const prev = previousEqualRange(r);
      const [current, previous, cost, count, byVirtualKey] = await Promise.all([
        getStats(r.since, r.until, "model"),
        getStats(prev.since, prev.until, "model"),
        getTimeSeries(r.since, r.until, "model", "cost_usd"),
        getTimeSeries(r.since, r.until, "model", "count"),
        getStats(r.since, r.until, "virtual_key"),
      ]);
      if (seq !== statsSeq) return;
      rows = current;
      prevRows = previous;
      costSeries = cost;
      countSeries = count;
      vkRows = byVirtualKey;
    } catch (e) {
      if (seq === statsSeq) pushToast(`Failed to load stats: ${e}`, "danger");
    } finally {
      if (seq === statsSeq) loading = false;
    }
  }

  $effect(() => {
    void range;
    loadStats();
  });
</script>

<div class="dashboard">
  <div class="toolbar">
    <h2>Dashboard</h2>
    <div class="toolbar-controls">
      <RefreshControl onRefresh={loadStats} loading={loading} />
      <TimeRangePicker bind:value={range} />
    </div>
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
    <StatBreakdownTable groupLabel="Model" rows={rows} />
  </Card>

  <Card title="By virtual key">
    <StatBreakdownTable groupLabel="Virtual key" rows={vkRows} resolveDisplayLabel={resolveVirtualKeyLabel} />
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

  .toolbar-controls {
    display: flex;
    align-items: center;
    gap: var(--space-3);
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
</style>
