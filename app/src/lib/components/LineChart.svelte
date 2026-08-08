<script lang="ts">
  import { scaleLinear } from "d3-scale";
  import { line, area, curveMonotoneX } from "d3-shape";
  import { CHART_COLORS } from "../chartColors";

  interface Props {
    buckets: string[];
    series: Record<string, number[]>;
    height?: number;
  }

  let { buckets, series, height = 200 }: Props = $props();

  const width = 600;
  const padding = { top: 12, right: 12, bottom: 24, left: 12 };

  const seriesNames = $derived(Object.keys(series));

  const maxY = $derived(Math.max(1, ...seriesNames.flatMap((name) => series[name] ?? [0])));

  const xScale = $derived(
    scaleLinear()
      .domain([0, Math.max(1, buckets.length - 1)])
      .range([padding.left, width - padding.right]),
  );

  const yScale = $derived(
    scaleLinear()
      .domain([0, maxY])
      .range([height - padding.bottom, padding.top]),
  );

  const lineGen = $derived(
    line<number>()
      .x((_, i) => xScale(i))
      .y((d) => yScale(d))
      .curve(curveMonotoneX),
  );

  const areaGen = $derived(
    area<number>()
      .x((_, i) => xScale(i))
      .y0(height - padding.bottom)
      .y1((d) => yScale(d))
      .curve(curveMonotoneX),
  );

  const tickIndices = $derived(
    buckets.length <= 1 ? [0] : [0, Math.floor((buckets.length - 1) / 2), buckets.length - 1],
  );
</script>

<svg viewBox="0 0 {width} {height}" preserveAspectRatio="none" class="chart">
  {#if buckets.length === 0}
    <text x={width / 2} y={height / 2} text-anchor="middle" class="empty">No data</text>
  {:else}
    {#each seriesNames as name, i (name)}
      {@const color = CHART_COLORS[i % CHART_COLORS.length]}
      {@const values = series[name] ?? []}
      <path d={areaGen(values) ?? ""} fill={color} fill-opacity="0.12" stroke="none" />
      <path d={lineGen(values) ?? ""} fill="none" stroke={color} stroke-width="2" />
    {/each}
    {#each tickIndices as idx (idx)}
      <text x={xScale(idx)} y={height - 6} text-anchor="middle" class="tick">{buckets[idx]}</text>
    {/each}
  {/if}
</svg>

<style>
  .chart {
    width: 100%;
    height: auto;
    display: block;
  }

  .tick {
    font-family: var(--font-mono);
    font-size: 10px;
    fill: var(--text-muted);
  }

  .empty {
    font-size: 13px;
    fill: var(--text-muted);
  }
</style>
