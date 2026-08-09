<script lang="ts">
  import Dropdown from "./Dropdown.svelte";

  interface Props {
    onRefresh: () => void;
    loading?: boolean;
  }

  let { onRefresh, loading = false }: Props = $props();

  const AUTO_OPTIONS = [
    { value: "off", label: "Auto: off" },
    { value: "5", label: "Every 5s" },
    { value: "10", label: "Every 10s" },
    { value: "30", label: "Every 30s" },
    { value: "60", label: "Every 60s" },
  ];

  let autoRefresh = $state("off");

  $effect(() => {
    if (autoRefresh === "off") return;
    const id = setInterval(onRefresh, Number(autoRefresh) * 1000);
    return () => clearInterval(id);
  });
</script>

<div class="refresh-control">
  <button type="button" class="icon-btn" class:spinning={loading} disabled={loading} onclick={onRefresh} aria-label="Refresh now" title="Refresh now">
    ↻
  </button>
  <div class="auto-select">
    <Dropdown bind:value={autoRefresh} options={AUTO_OPTIONS} />
  </div>
</div>

<style>
  .refresh-control {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: var(--radius-control);
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text-muted);
    font-size: 15px;
    line-height: 1;
    cursor: pointer;
    flex-shrink: 0;
  }

  .icon-btn:not(:disabled):hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  .icon-btn:disabled {
    cursor: not-allowed;
    opacity: 0.6;
  }

  .icon-btn.spinning {
    animation: spin 0.8s linear infinite;
  }

  .auto-select {
    width: 110px;
  }

  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }
</style>
