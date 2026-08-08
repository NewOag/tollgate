<script lang="ts">
  interface Props {
    value: string;
    type?: "text" | "password";
    placeholder?: string;
    disabled?: boolean;
  }

  let { value = $bindable(), type = "text", placeholder = "", disabled = false }: Props = $props();
</script>

<input class="input" {type} {placeholder} {disabled} bind:value />

<style>
  .input {
    width: 100%;
    height: 32px;
    padding: 0 var(--space-3);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    color: var(--text);
    font-size: 13px;
  }

  .input::placeholder {
    color: var(--text-muted);
  }

  .input:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
