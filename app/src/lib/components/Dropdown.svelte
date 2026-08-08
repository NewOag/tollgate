<script lang="ts">
  export interface DropdownOption {
    value: string;
    label: string;
  }

  interface Props {
    options: DropdownOption[];
    value: string;
    disabled?: boolean;
    placeholder?: string;
  }

  let { value = $bindable(), options, disabled = false, placeholder = "" }: Props = $props();

  let root: HTMLDivElement | undefined;
  let open = $state(false);
  let highlightIndex = $state(-1);

  const selected = $derived(options.find((o) => o.value === value));

  function toggle() {
    if (disabled) return;
    open = !open;
    if (open) highlightIndex = options.findIndex((o) => o.value === value);
  }

  function select(v: string) {
    value = v;
    open = false;
  }

  function onWindowClick(e: MouseEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      open = false;
      return;
    }
    if (!open) {
      if (e.key === "Enter" || e.key === " " || e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        toggle();
      }
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      highlightIndex = Math.min(highlightIndex + 1, options.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      highlightIndex = Math.max(highlightIndex - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (highlightIndex >= 0 && highlightIndex < options.length) select(options[highlightIndex].value);
    }
  }
</script>

<svelte:window onclick={onWindowClick} />

<div class="dropdown" bind:this={root}>
  <button type="button" class="trigger" class:open {disabled} onclick={toggle} onkeydown={onKeydown}>
    <span class="trigger-label" class:placeholder={!selected}>{selected?.label ?? placeholder}</span>
    <span class="chevron" aria-hidden="true">▾</span>
  </button>

  {#if open}
    <ul class="panel" role="listbox">
      {#each options as opt, i (opt.value)}
        <li
          role="option"
          aria-selected={opt.value === value}
          class:selected={opt.value === value}
          class:highlighted={i === highlightIndex}
          onmouseenter={() => (highlightIndex = i)}
          onclick={() => select(opt.value)}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") select(opt.value);
          }}
        >
          {opt.label}
        </li>
      {/each}
      {#if options.length === 0}
        <li class="empty">No options</li>
      {/if}
    </ul>
  {/if}
</div>

<style>
  .dropdown {
    position: relative;
    width: 100%;
  }

  .trigger {
    width: 100%;
    height: 32px;
    padding: 0 var(--space-3);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    color: var(--text);
    font-size: 13px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    cursor: pointer;
  }

  .trigger.open {
    border-color: var(--accent);
  }

  .trigger:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .trigger-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }

  .trigger-label.placeholder {
    color: var(--text-muted);
  }

  .chevron {
    color: var(--text-muted);
    flex-shrink: 0;
    font-size: 10px;
  }

  .panel {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    z-index: 20;
    margin: 0;
    padding: var(--space-1);
    list-style: none;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    box-shadow: 0 8px 24px color-mix(in srgb, #000000 40%, transparent);
    max-height: 260px;
    overflow-y: auto;
  }

  .panel li {
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-control);
    font-size: 13px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .panel li.highlighted {
    background: var(--surface-hover);
  }

  .panel li.selected {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--accent);
  }

  .panel li.empty {
    color: var(--text-muted);
    cursor: default;
  }
</style>
