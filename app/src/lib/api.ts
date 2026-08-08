// Typed wrappers around `invoke` — one function per Tauri command in
// rustgate/app/src-tauri/src/commands.rs (all declared with
// `rename_all = "snake_case"`, so these argument keys match the Rust
// parameter names exactly, no camelCase translation to keep in sync).

import { invoke } from "@tauri-apps/api/core";
import type {
  Config,
  GatewayStatus,
  GroupBy,
  Metric,
  ModelPrice,
  RequestListFilter,
  RequestRecord,
  RequestSummary,
  Route,
  StatRow,
  TimeSeriesResult,
} from "./types";

export function getConfig(): Promise<Config> {
  return invoke("get_config");
}

export function saveRoute(route: Route): Promise<void> {
  return invoke("save_route", { route });
}

export function deleteRoute(name: string): Promise<void> {
  return invoke("delete_route", { name });
}

export function savePricing(model: string, price: ModelPrice): Promise<void> {
  return invoke("save_pricing", { model, price });
}

export function deletePricing(model: string): Promise<void> {
  return invoke("delete_pricing", { model });
}

export function saveGeneralSettings(max_body_bytes: number, shutdown_timeout: string): Promise<void> {
  return invoke("save_general_settings", { max_body_bytes, shutdown_timeout });
}

export function getStats(since: string, until: string, group_by: GroupBy): Promise<StatRow[]> {
  return invoke("get_stats", { since, until, group_by });
}

export function getTimeSeries(
  since: string,
  until: string,
  group_by: GroupBy,
  metric: Metric,
  bucket?: string,
): Promise<TimeSeriesResult> {
  return invoke("get_time_series", { since, until, group_by, metric, bucket: bucket ?? null });
}

export function listRequests(filter: RequestListFilter): Promise<[RequestSummary[], number]> {
  return invoke("list_requests", { args: filter });
}

export function getRequest(id: number): Promise<RequestRecord | null> {
  return invoke("get_request", { id });
}

export function listDistinctModels(): Promise<string[]> {
  return invoke("list_distinct_models");
}

export function listDistinctVirtualKeyLabels(): Promise<string[]> {
  return invoke("list_distinct_virtual_key_labels");
}

export function listDistinctSessionIds(): Promise<string[]> {
  return invoke("list_distinct_session_ids");
}

export function getGatewayStatus(): Promise<GatewayStatus> {
  return invoke("get_gateway_status");
}
