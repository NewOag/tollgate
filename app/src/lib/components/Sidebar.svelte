<script lang="ts">
  import { currentView, type View } from "../view";

  const items: { view: View; label: string }[] = [
    { view: "dashboard", label: "Dashboard" },
    { view: "requests", label: "Requests" },
    { view: "routes", label: "Routes & Keys" },
    { view: "pricing", label: "Pricing" },
    { view: "settings", label: "Settings" },
  ];
</script>

<nav class="sidebar">
  <div class="brand">Tollgate</div>
  <ul>
    {#each items as item (item.view)}
      <li>
        <button
          class="nav-item"
          class:active={$currentView === item.view}
          onclick={() => currentView.set(item.view)}
        >
          {item.label}
        </button>
      </li>
    {/each}
  </ul>
</nav>

<style>
  .sidebar {
    width: 200px;
    flex-shrink: 0;
    background: var(--surface);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    padding: var(--space-4) var(--space-2);
  }

  .brand {
    font-size: 14px;
    font-weight: 700;
    letter-spacing: 0.02em;
    color: var(--accent);
    padding: 0 var(--space-3) var(--space-4);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .nav-item {
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-control);
    color: var(--text-muted);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
  }

  .nav-item:hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  .nav-item.active {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--accent);
  }
</style>
