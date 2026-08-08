<script lang="ts">
  import { onMount } from "svelte";
  import { currentView } from "$lib/view";
  import { getGatewayStatus } from "$lib/api";
  import type { GatewayStatus } from "$lib/types";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ToastStack from "$lib/components/Toast.svelte";
  import Dashboard from "$lib/views/Dashboard.svelte";
  import Requests from "$lib/views/Requests.svelte";
  import Routes from "$lib/views/Routes.svelte";
  import Pricing from "$lib/views/Pricing.svelte";
  import Settings from "$lib/views/Settings.svelte";

  let status = $state<GatewayStatus | null>(null);

  onMount(async () => {
    status = await getGatewayStatus();
  });
</script>

<div class="shell">
  <Sidebar />
  <div class="main">
    {#if status?.bind_error}
      <div class="banner">Gateway failed to bind {status.listen_addr}: {status.bind_error}</div>
    {/if}
    <div class="content">
      {#if $currentView === "dashboard"}
        <Dashboard />
      {:else if $currentView === "requests"}
        <Requests />
      {:else if $currentView === "routes"}
        <Routes />
      {:else if $currentView === "pricing"}
        <Pricing />
      {:else if $currentView === "settings"}
        <Settings />
      {/if}
    </div>
  </div>
</div>
<ToastStack />

<style>
  .shell {
    display: flex;
    height: 100vh;
  }

  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .banner {
    background: color-mix(in srgb, var(--danger) 15%, transparent);
    color: var(--danger);
    padding: var(--space-2) var(--space-4);
    font-size: 13px;
    border-bottom: 1px solid var(--danger);
  }

  .content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
  }
</style>
