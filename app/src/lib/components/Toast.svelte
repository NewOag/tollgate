<script lang="ts">
  import { toasts, dismissToast } from "../toast";
</script>

<div class="toast-stack">
  {#each $toasts as t (t.id)}
    <div class="toast {t.kind}">
      <span>{t.message}</span>
      <button class="dismiss" onclick={() => dismissToast(t.id)} aria-label="Dismiss">&times;</button>
    </div>
  {/each}
</div>

<style>
  .toast-stack {
    position: fixed;
    bottom: var(--space-6);
    right: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    z-index: 200;
  }

  .toast {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 240px;
    max-width: 360px;
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-control);
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text);
    font-size: 13px;
    box-shadow: 0 8px 24px color-mix(in srgb, #000000 40%, transparent);
  }

  .toast.success {
    border-color: var(--success);
  }

  .toast.danger {
    border-color: var(--danger);
  }

  .toast span {
    flex: 1;
  }

  .dismiss {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
    padding: 0;
  }

  .dismiss:hover {
    color: var(--text);
  }
</style>
