<script lang="ts">
  import Modal from "./Modal.svelte";
  import Button from "./Button.svelte";

  interface Props {
    title: string;
    message: string;
    confirmLabel?: string;
    onconfirm: () => void;
    oncancel: () => void;
  }

  let { title, message, confirmLabel = "Delete", onconfirm, oncancel }: Props = $props();
</script>

<Modal {title} onclose={oncancel}>
  {#snippet children()}
    <p>{message}</p>
  {/snippet}
  {#snippet footer()}
    <Button variant="secondary" onclick={oncancel}>Cancel</Button>
    <Button variant="danger" onclick={onconfirm}>{confirmLabel}</Button>
  {/snippet}
</Modal>
