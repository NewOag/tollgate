// Mirrors the Rust DTOs in rustgate/src/{config.rs,store/*.rs} and
// rustgate/app/src-tauri/src/commands.rs. Field names are snake_case to
// match serde's default (no rename attributes on either side).

export interface RealKeyEntry {
  label: string;
  value: string;
}

export interface KeyEntry {
  value: string;
  label: string;
  real_key: string;
}

export interface Route {
  name: string;
  format: "openai" | "anthropic" | string;
  upstream: string;
  real_keys: RealKeyEntry[];
  keys: KeyEntry[];
}

export interface ModelPrice {
  input_per_mtok: number;
  output_per_mtok: number;
  cached_input_per_mtok: number;
  cache_write_per_mtok: number;
}

export interface Config {
  listen: string;
  db_path: string;
  routes: Route[];
  max_body_bytes: number;
  shutdown_timeout: string;
  pricing: Record<string, ModelPrice>;
}

export type GroupBy = "model" | "route" | "virtual_key" | "real_key";
export type Metric = "count" | "total_tokens" | "cost_usd";
export type SinceWindow = "1h" | "24h" | "7d" | "30d";

// Both fields are RFC3339 ISO8601 absolute timestamps, computed
// client-side from either a preset ("now minus 24h") or a custom range
// picker — the Tauri commands take these directly, no duration parsing.
export interface TimeRange {
  since: string;
  until: string;
}

export interface StatRow {
  // For virtual_key grouping this is the key's stable *value*, not its
  // label (so a renamed key's old/new rows merge into one group) — use
  // `group_label` for display, resolving it against the current config
  // first and falling back to this snapshot label if the key was deleted.
  // For every other group_by, this already *is* the display label.
  group: string;
  group_label: string;
  count: number;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  cache_creation_tokens: number;
  cache_read_tokens: number;
  cache_hit_rate: number;
  avg_latency_ms: number;
  errors: number;
  total_cost_usd: number;
}

export interface TimeSeriesResult {
  buckets: string[];
  series: Record<string, number[]>;
}

export interface RequestSummary {
  id: number;
  timestamp: string;
  route: string;
  virtual_key_label: string;
  real_key_label: string;
  format: string;
  model: string;
  path: string;
  stream: boolean;
  status_code: number;
  latency_ms: number;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  cost_usd: number;
  error: string;
}

export interface RequestRecord extends RequestSummary {
  cache_creation_tokens: number;
  cache_read_tokens: number;
  request_body: string;
  response_body: string;
  virtual_key_value: string;
  session_id: string;
  request_headers: string;
  response_headers: string;
}

export interface RequestListFilter {
  since: string;
  until: string;
  route?: string;
  model?: string;
  virtual_key_label?: string;
  real_key_label?: string;
  session_id?: string;
  limit: number;
  offset: number;
}

export interface GatewayStatus {
  listen_addr: string;
  config_path: string;
  db_path: string;
  bind_error: string | null;
  tls_enabled: boolean;
  https_url: string | null;
  mdns_url: string | null;
  tls_fingerprint: string | null;
}
