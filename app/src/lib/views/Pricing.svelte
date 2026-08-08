<script lang="ts">
  import { onMount } from "svelte";
  import { getConfig, savePricing, deletePricing } from "../api";
  import type { ModelPrice } from "../types";
  import { pushToast } from "../toast";
  import Card from "../components/Card.svelte";
  import Button from "../components/Button.svelte";
  import Input from "../components/Input.svelte";
  import NumberInput from "../components/NumberInput.svelte";
  import Table from "../components/Table.svelte";

  function emptyPrice(): ModelPrice {
    return { input_per_mtok: 0, output_per_mtok: 0, cached_input_per_mtok: 0, cache_write_per_mtok: 0 };
  }

  let drafts = $state<Record<string, ModelPrice>>({});
  let modelOrder = $state<string[]>([]);

  let newModel = $state("");
  let newPrice = $state<ModelPrice>(emptyPrice());

  async function load() {
    try {
      const cfg = await getConfig();
      drafts = Object.fromEntries(Object.entries(cfg.pricing).map(([m, p]) => [m, { ...p }]));
      modelOrder = Object.keys(drafts).sort();
    } catch (e) {
      pushToast(`Failed to load pricing: ${e}`, "danger");
    }
  }

  onMount(load);

  async function saveRow(model: string) {
    try {
      await savePricing(model, drafts[model]);
      pushToast(`Saved pricing for "${model}"`);
    } catch (e) {
      pushToast(`Failed to save pricing: ${e}`, "danger");
    }
  }

  async function removeRow(model: string) {
    if (!confirm(`Delete pricing for "${model}"?`)) return;
    try {
      await deletePricing(model);
      pushToast(`Deleted pricing for "${model}"`);
      await load();
    } catch (e) {
      pushToast(`Failed to delete pricing: ${e}`, "danger");
    }
  }

  async function addModel() {
    const model = newModel.trim();
    if (!model) {
      pushToast("Model name is required.", "danger");
      return;
    }
    if (drafts[model]) {
      pushToast(`Pricing for "${model}" already exists.`, "danger");
      return;
    }
    try {
      await savePricing(model, newPrice);
      pushToast(`Added pricing for "${model}"`);
      newModel = "";
      newPrice = emptyPrice();
      await load();
    } catch (e) {
      pushToast(`Failed to add pricing: ${e}`, "danger");
    }
  }
</script>

<div class="pricing-page">
  <div class="toolbar">
    <h2>Pricing</h2>
  </div>

  <Card>
    <Table>
      <thead>
        <tr>
          <th>Model</th>
          <th>Input / Mtok</th>
          <th>Output / Mtok</th>
          <th>Cached input / Mtok</th>
          <th>Cache write / Mtok</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each modelOrder as model (model)}
          <tr>
            <td>{model}</td>
            <td><NumberInput bind:value={drafts[model].input_per_mtok} min={0} step={0.01} /></td>
            <td><NumberInput bind:value={drafts[model].output_per_mtok} min={0} step={0.01} /></td>
            <td><NumberInput bind:value={drafts[model].cached_input_per_mtok} min={0} step={0.01} /></td>
            <td><NumberInput bind:value={drafts[model].cache_write_per_mtok} min={0} step={0.01} /></td>
            <td class="row-actions">
              <Button variant="secondary" onclick={() => saveRow(model)}>Save</Button>
              <Button variant="danger" onclick={() => removeRow(model)}>Delete</Button>
            </td>
          </tr>
        {/each}
        {#if modelOrder.length === 0}
          <tr><td colspan="6" class="empty">No pricing configured yet.</td></tr>
        {/if}
      </tbody>
    </Table>
  </Card>

  <Card title="Add model pricing">
    <div class="new-row">
      <Input bind:value={newModel} placeholder="model name" />
      <NumberInput bind:value={newPrice.input_per_mtok} min={0} step={0.01} />
      <NumberInput bind:value={newPrice.output_per_mtok} min={0} step={0.01} />
      <NumberInput bind:value={newPrice.cached_input_per_mtok} min={0} step={0.01} />
      <NumberInput bind:value={newPrice.cache_write_per_mtok} min={0} step={0.01} />
      <Button variant="primary" onclick={addModel}>+ Add</Button>
    </div>
  </Card>
</div>

<style>
  .pricing-page {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .toolbar h2 {
    font-size: 18px;
    font-weight: 600;
  }

  .row-actions {
    display: flex;
    gap: var(--space-2);
    white-space: nowrap;
  }

  .empty {
    text-align: center;
    color: var(--text-muted);
    padding: var(--space-4);
  }

  .new-row {
    display: grid;
    grid-template-columns: 1.5fr 1fr 1fr 1fr 1fr auto;
    gap: var(--space-3);
    align-items: center;
  }
</style>
