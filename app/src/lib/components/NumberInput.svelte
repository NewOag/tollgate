<script lang="ts">
  interface Props {
    value: number;
    min?: number;
    step?: number;
    disabled?: boolean;
  }

  let { value = $bindable(), min, step = 1, disabled = false }: Props = $props();
</script>

<input class="num-input num" type="number" {min} {step} {disabled} bind:value />

<style>
  .num-input {
    width: 100%;
    height: 32px;
    padding: 0 var(--space-3);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    color: var(--text);
    font-size: 13px;
  }

  .num-input:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
