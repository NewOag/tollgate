import type { TimeRange } from "./types";

export const PRESETS: { key: string; label: string; ms: number }[] = [
  { key: "1h", label: "Last hour", ms: 3600_000 },
  { key: "24h", label: "Last 24 hours", ms: 24 * 3600_000 },
  { key: "7d", label: "Last 7 days", ms: 7 * 24 * 3600_000 },
  { key: "30d", label: "Last 30 days", ms: 30 * 24 * 3600_000 },
];

export function presetRange(ms: number, now: Date = new Date()): TimeRange {
  return { since: new Date(now.getTime() - ms).toISOString(), until: now.toISOString() };
}

export function defaultRange(): TimeRange {
  return presetRange(PRESETS[1].ms);
}

// The equal-length window immediately preceding `range`, for
// period-over-period comparisons — works for arbitrary custom ranges,
// not just the fixed presets.
export function previousEqualRange(range: TimeRange): TimeRange {
  const since = new Date(range.since).getTime();
  const until = new Date(range.until).getTime();
  const span = until - since;
  return { since: new Date(since - span).toISOString(), until: range.since };
}
