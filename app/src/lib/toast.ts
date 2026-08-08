import { writable } from "svelte/store";

export interface Toast {
  id: number;
  kind: "success" | "danger";
  message: string;
}

export const toasts = writable<Toast[]>([]);

let nextId = 1;

export function pushToast(message: string, kind: Toast["kind"] = "success", timeoutMs = 4000) {
  const id = nextId++;
  toasts.update((t) => [...t, { id, kind, message }]);
  setTimeout(() => dismissToast(id), timeoutMs);
}

export function dismissToast(id: number) {
  toasts.update((t) => t.filter((x) => x.id !== id));
}
