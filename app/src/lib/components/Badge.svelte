<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Variant } from "../format";

  interface Props {
    variant?: Variant;
    children: Snippet;
  }

  let { variant = "muted", children }: Props = $props();
</script>

<span class="badge {variant}">
  {@render children()}
</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    height: 20px;
    padding: 0 var(--space-2);
    border-radius: var(--radius-pill);
    font-size: 12px;
    font-weight: 500;
    line-height: 1;
    white-space: nowrap;
  }

  .success {
    background: color-mix(in srgb, var(--success) 15%, transparent);
    color: var(--success);
  }

  .warning {
    background: color-mix(in srgb, var(--warning) 15%, transparent);
    color: var(--warning);
  }

  .danger {
    background: color-mix(in srgb, var(--danger) 15%, transparent);
    color: var(--danger);
  }

  .accent {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--accent);
  }

  .muted {
    background: color-mix(in srgb, var(--text-muted) 15%, transparent);
    color: var(--text-muted);
  }
</style>
