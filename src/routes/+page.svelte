<script lang="ts">
  import { currentPage, searchQuery } from '$lib/stores';
  import TopBar from '$lib/components/TopBar.svelte';
  import Editor from '$lib/components/Editor.svelte';
  import Dashboard from '$lib/components/Dashboard.svelte';
  import SearchResults from '$lib/components/SearchResults.svelte';
</script>

{#if $searchQuery}
  <SearchResults />
{:else if $currentPage}
  <TopBar />
  {#key $currentPage.id}
    <Editor />
  {/key}
{:else}
  <Dashboard />
{/if}
