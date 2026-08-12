import { defaultRange, DEFAULT_PRESET_KEY } from "./timeRange";

// Module-level $state survives view switches (the view components
// unmount/remount via {#if} in +page.svelte) without needing
// localStorage — it resets only on a full app reload/restart.

export const dashboardFilters = $state({
  range: defaultRange(),
  activePresetKey: DEFAULT_PRESET_KEY,
});

export const requestsFilters = $state({
  range: defaultRange(),
  activePresetKey: DEFAULT_PRESET_KEY,
  routeFilter: "",
  modelFilter: "",
  vkFilter: "",
  sessionFilter: "",
  page: 0,
});
