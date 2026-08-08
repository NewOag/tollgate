<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    title?: string;
    children: Snippet;
    actions?: Snippet;
  }

  let { title, children, actions }: Props = $props();
</script>

<div class="card">
  {#if title || actions}
    <div class="card-header">
      {#if title}<h3>{title}</h3>{/if}
      {#if actions}<div class="actions">{@render actions()}</div>{/if}
    </div>
  {/if}
  <div class="card-body">
    {@render children()}
  </div>
</div>

<style>
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
  }

  .card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-4) 0;
  }

  .card-header h3 {
    font-size: 14px;
    font-weight: 600;
    color: var(--text);
  }

  .card-body {
    padding: var(--space-4);
  }
</style>
