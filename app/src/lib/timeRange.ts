import type { TimeRange } from "./types";

export interface Preset {
  key: string;
  label: string;
  since: (now: Date) => Date;
}

function startOfDay(now: Date): Date {
  const d = new Date(now);
  d.setHours(0, 0, 0, 0);
  return d;
}

function startOfWeek(now: Date): Date {
  const d = startOfDay(now);
  const daysSinceMonday = (d.getDay() + 6) % 7;
  d.setDate(d.getDate() - daysSinceMonday);
  return d;
}

export const PRESETS: Preset[] = [
  { key: "today", label: "Today", since: startOfDay },
  { key: "week", label: "This week", since: startOfWeek },
  { key: "1h", label: "Last hour", since: (now) => new Date(now.getTime() - 3600_000) },
  { key: "24h", label: "Last 24 hours", since: (now) => new Date(now.getTime() - 24 * 3600_000) },
  { key: "7d", label: "Last 7 days", since: (now) => new Date(now.getTime() - 7 * 24 * 3600_000) },
  { key: "30d", label: "Last 30 days", since: (now) => new Date(now.getTime() - 30 * 24 * 3600_000) },
];

export const DEFAULT_PRESET_KEY = "24h";

export function presetRangeForKey(key: string, now: Date = new Date()): TimeRange {
  const preset = PRESETS.find((p) => p.key === key) ?? PRESETS.find((p) => p.key === DEFAULT_PRESET_KEY)!;
  return { since: preset.since(now).toISOString(), until: now.toISOString() };
}

export function defaultRange(): TimeRange {
  return presetRangeForKey(DEFAULT_PRESET_KEY);
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
