import { writable } from "svelte/store";

export type View = "dashboard" | "requests" | "routes" | "pricing" | "settings";

export const currentView = writable<View>("dashboard");
