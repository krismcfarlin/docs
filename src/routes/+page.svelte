<script lang="ts">
  import TopBar from '$lib/components/TopBar.svelte';
  import Editor from '$lib/components/Editor.svelte';
  import Dashboard from '$lib/components/Dashboard.svelte';
  import { currentPage } from '$lib/stores';
</script>

{#if $currentPage}
  <TopBar />
  {#key $currentPage.id}
    <Editor />
  {/key}
{:else}
  <Dashboard />
{/if}
