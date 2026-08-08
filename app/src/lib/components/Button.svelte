<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    variant?: "primary" | "secondary" | "ghost" | "danger";
    type?: "button" | "submit";
    disabled?: boolean;
    onclick?: (e: MouseEvent) => void;
    children: Snippet;
  }

  let { variant = "secondary", type = "button", disabled = false, onclick, children }: Props = $props();
</script>

<button class="btn {variant}" {type} {disabled} {onclick}>
  {@render children()}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    height: 32px;
    padding: 0 var(--space-3);
    border-radius: var(--radius-control);
    border: 1px solid transparent;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 0.12s, border-color 0.12s, color 0.12s;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .primary {
    background: var(--accent);
    color: var(--bg);
  }

  .primary:not(:disabled):hover {
    background: var(--accent-hover);
  }

  .secondary {
    background: var(--surface);
    border-color: var(--border);
    color: var(--text);
  }

  .secondary:not(:disabled):hover {
    background: var(--surface-hover);
  }

  .ghost {
    background: transparent;
    color: var(--text-muted);
  }

  .ghost:not(:disabled):hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  .danger {
    background: transparent;
    border-color: var(--danger);
    color: var(--danger);
  }

  .danger:not(:disabled):hover {
    background: color-mix(in srgb, var(--danger) 15%, transparent);
  }
</style>
