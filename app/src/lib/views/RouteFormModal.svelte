<script lang="ts">
  import { saveRoute } from "../api";
  import type { Route, RealKeyEntry, KeyEntry } from "../types";
  import { pushToast } from "../toast";
  import Modal from "../components/Modal.svelte";
  import Input from "../components/Input.svelte";
  import Dropdown from "../components/Dropdown.svelte";
  import Button from "../components/Button.svelte";

  interface Props {
    route: Route | null;
    existingNames: string[];
    onclose: () => void;
    onsaved: () => void;
  }

  let { route, existingNames, onclose, onsaved }: Props = $props();

  const isEdit = route !== null;

  let name = $state(route?.name ?? "");
  let format = $state(route?.format ?? "openai");
  let upstream = $state(route?.upstream ?? "");
  let realKeys = $state<RealKeyEntry[]>(route ? route.real_keys.map((k) => ({ ...k })) : [{ label: "", value: "" }]);
  let virtualKeys = $state<KeyEntry[]>(
    route ? route.keys.map((k) => ({ ...k })) : [{ label: "", value: "", real_key: "" }],
  );
  let errors = $state<string[]>([]);
  let saving = $state(false);

  function addRealKey() {
    realKeys = [...realKeys, { label: "", value: "" }];
  }

  function removeRealKey(i: number) {
    realKeys = realKeys.filter((_, idx) => idx !== i);
  }

  function addVirtualKey() {
    virtualKeys = [...virtualKeys, { label: "", value: "", real_key: realKeys[0]?.label ?? "" }];
  }

  function removeVirtualKey(i: number) {
    virtualKeys = virtualKeys.filter((_, idx) => idx !== i);
  }

  function generateSecret(): string {
    return `vk-${crypto.randomUUID().replace(/-/g, "")}`;
  }

  function validate(): string[] {
    const errs: string[] = [];
    const trimmedName = name.trim();
    if (!trimmedName) errs.push("Route name is required.");
    if (!isEdit && existingNames.includes(trimmedName)) errs.push("A route with this name already exists.");
    if (!upstream.trim()) errs.push("Upstream URL is required.");

    if (realKeys.length === 0) errs.push("At least one real key is required.");
    const rkLabels = new Set<string>();
    for (const rk of realKeys) {
      if (!rk.label.trim() || !rk.value.trim()) errs.push("Every real key needs a label and a value.");
      else if (rkLabels.has(rk.label)) errs.push(`Duplicate real key label "${rk.label}".`);
      rkLabels.add(rk.label);
    }

    if (virtualKeys.length === 0) errs.push("At least one virtual key is required.");
    const vkLabels = new Set<string>();
    for (const vk of virtualKeys) {
      if (!vk.label.trim() || !vk.value.trim()) errs.push("Every virtual key needs a label and a value.");
      else if (vkLabels.has(vk.label)) errs.push(`Duplicate virtual key label "${vk.label}".`);
      if (!rkLabels.has(vk.real_key)) errs.push(`Virtual key "${vk.label || "(unnamed)"}" must point to one of this route's real keys.`);
      vkLabels.add(vk.label);
    }

    return errs;
  }

  async function submit() {
    errors = validate();
    if (errors.length > 0) return;
    saving = true;
    try {
      await saveRoute({
        name: name.trim(),
        format,
        upstream: upstream.trim(),
        real_keys: realKeys,
        keys: virtualKeys,
      });
      pushToast(isEdit ? `Updated route "${name}"` : `Created route "${name}"`);
      onsaved();
    } catch (e) {
      pushToast(`Failed to save route: ${e}`, "danger");
    } finally {
      saving = false;
    }
  }
</script>

<Modal title={isEdit ? `Edit route "${route?.name}"` : "New route"} {onclose}>
  <div class="form-row">
    <label for="rf-name">Name</label>
    <Input bind:value={name} disabled={isEdit} placeholder="my-route" />
  </div>
  <div class="form-row">
    <label for="rf-format">Format</label>
    <Dropdown
      bind:value={format}
      options={[
        { value: "openai", label: "openai" },
        { value: "anthropic", label: "anthropic" },
        { value: "openai_responses", label: "openai_responses" },
      ]}
    />
  </div>
  <div class="form-row">
    <label for="rf-upstream">Upstream</label>
    <Input bind:value={upstream} placeholder="https://api.openai.com" />
  </div>

  <div class="key-editor">
    <div class="key-editor-header">
      <h4>Real keys</h4>
      <Button variant="ghost" onclick={addRealKey}>+ Add</Button>
    </div>
    {#if realKeys.length > 0}
      <div class="key-editor-columns">
        <span>Label</span>
        <span>Secret value</span>
      </div>
    {/if}
    {#each realKeys as rk, i (i)}
      <div class="key-editor-row">
        <Input bind:value={rk.label} placeholder="label" />
        <Input bind:value={rk.value} placeholder="secret value" />
        <Button variant="ghost" onclick={() => (rk.value = generateSecret())}>Gen</Button>
        <Button variant="ghost" onclick={() => removeRealKey(i)}>✕</Button>
      </div>
    {/each}
  </div>

  <div class="key-editor">
    <div class="key-editor-header">
      <h4>Virtual keys</h4>
      <Button variant="ghost" onclick={addVirtualKey}>+ Add</Button>
    </div>
    {#if virtualKeys.length > 0}
      <div class="key-editor-columns virtual">
        <span>Label</span>
        <span>Real key</span>
        <span>Secret value</span>
      </div>
    {/if}
    {#each virtualKeys as vk, i (i)}
      <div class="key-editor-row virtual">
        <Input bind:value={vk.label} placeholder="label" />
        <Dropdown
          bind:value={vk.real_key}
          placeholder="select real key"
          options={realKeys.map((rk) => ({ value: rk.label, label: rk.label }))}
        />
        <Input bind:value={vk.value} placeholder="secret value" />
        <Button variant="ghost" onclick={() => (vk.value = generateSecret())}>Gen</Button>
        <Button variant="ghost" onclick={() => removeVirtualKey(i)}>✕</Button>
      </div>
    {/each}
  </div>

  {#if errors.length > 0}
    <ul class="errors">
      {#each errors as err (err)}
        <li>{err}</li>
      {/each}
    </ul>
  {/if}

  {#snippet footer()}
    <Button variant="secondary" onclick={onclose}>Cancel</Button>
    <Button variant="primary" onclick={submit} disabled={saving}>{saving ? "Saving…" : "Save"}</Button>
  {/snippet}
</Modal>

<style>
  .form-row {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .form-row label {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .key-editor {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border-top: 1px solid var(--border);
    padding-top: var(--space-3);
  }

  .key-editor-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .key-editor-header h4 {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .key-editor-columns {
    display: grid;
    grid-template-columns: 1fr 1fr auto auto;
    gap: var(--space-2);
  }

  .key-editor-columns.virtual {
    grid-template-columns: 1fr 1fr 1fr auto auto;
  }

  .key-editor-columns span {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .key-editor-row {
    display: grid;
    grid-template-columns: 1fr 1fr auto auto;
    gap: var(--space-2);
    align-items: center;
  }

  .key-editor-row.virtual {
    grid-template-columns: 1fr 1fr 1fr auto auto;
  }

  .errors {
    margin: 0;
    padding: var(--space-3);
    background: color-mix(in srgb, var(--danger) 15%, transparent);
    border-radius: var(--radius-control);
    color: var(--danger);
    font-size: 12px;
    list-style: disc;
    padding-left: var(--space-6);
  }
</style>
