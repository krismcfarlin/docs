<script lang="ts">
  import { searchQuery, searchResults, currentPage, currentVersion, pages, spaces, currentSpace } from '$lib/stores';
  import { getPageVersion } from '$lib/api';
  import { X, FileText, Search } from 'lucide-svelte';

  function clearSearch(): void {
    $searchQuery = '';
    $searchResults = [];
  }

  async function openResult(pageId: string): Promise<void> {
    const page = $pages.find(p => p.id === pageId);
    if (!page) return;
    $currentPage = page;
    const spaceId = $currentSpace?.id ?? page.space_id;
    try {
      $currentVersion = await getPageVersion(page.id, spaceId);
    } catch (e) {
      console.error('[SearchResults] failed to load version for page', page.id, e);
    }
    clearSearch();
  }

  function spaceName(spaceId: string): string {
    return $spaces.find(s => s.id === spaceId)?.name ?? '';
  }

  function scorePercent(score: number): string {
    return `${Math.round(score * 100)}%`;
  }
</script>

<div class="flex flex-col flex-1 min-h-0 overflow-hidden" style="background: var(--color-bg);">
  <!-- Header -->
  <div
    class="flex items-center gap-3 px-6 py-4 flex-shrink-0 border-b"
    style="background: var(--color-surface); border-color: var(--color-border);"
  >
    <Search size={16} style="color: var(--color-on-muted); flex-shrink: 0;" />
    <div class="flex-1 min-w-0">
      <span class="text-sm font-semibold" style="color: var(--color-on-surface);">
        Search: <span style="color: var(--color-primary);">{$searchQuery}</span>
      </span>
      <span class="ml-2 text-xs" style="color: var(--color-on-muted);">
        {$searchResults.length} result{$searchResults.length !== 1 ? 's' : ''}
      </span>
    </div>
    <button
      onclick={clearSearch}
      class="w-7 h-7 flex items-center justify-center rounded-lg transition-colors flex-shrink-0"
      style="color: var(--color-on-muted);"
      onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
      onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
      title="Clear search"
    >
      <X size={14} />
    </button>
  </div>

  <!-- Results -->
  <div class="flex-1 overflow-y-auto px-6 py-4">
    {#if $searchResults.length === 0}
      <div class="flex flex-col items-center justify-center h-48 gap-3">
        <Search size={32} style="color: var(--color-on-muted); opacity: 0.3;" />
        <p class="text-sm" style="color: var(--color-on-muted);">No indexed pages match this query.</p>
        <p class="text-xs" style="color: var(--color-on-muted); opacity: 0.6;">Freeze a page to include it in search.</p>
      </div>
    {:else}
      <div class="grid gap-3 max-w-4xl mx-auto" style="grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));">
        {#each $searchResults as result (result.page_id)}
          {@const space = spaceName($pages.find(p => p.id === result.page_id)?.space_id ?? '')}
          <button
            onclick={() => openResult(result.page_id)}
            class="text-left flex flex-col gap-2.5 p-4 rounded-xl transition-all"
            style="background: var(--color-surface); border: 1px solid var(--color-border);"
            onmouseenter={(e) => {
              (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)';
              (e.currentTarget as HTMLElement).style.borderColor = 'var(--color-primary)';
            }}
            onmouseleave={(e) => {
              (e.currentTarget as HTMLElement).style.background = 'var(--color-surface)';
              (e.currentTarget as HTMLElement).style.borderColor = 'var(--color-border)';
            }}
          >
            <!-- Title row -->
            <div class="flex items-start gap-2">
              <FileText size={14} style="color: var(--color-primary); flex-shrink: 0; margin-top: 1px;" />
              <span
                class="flex-1 text-sm font-semibold leading-snug"
                style="color: var(--color-on-surface);"
              >{result.title}</span>
            </div>

            <!-- Snippet -->
            {#if result.snippet}
              <p
                class="text-xs leading-relaxed line-clamp-3"
                style="color: var(--color-on-muted);"
              >{result.snippet}</p>
            {/if}

            <!-- Footer: space name + score bar -->
            <div class="flex items-center justify-between gap-2 mt-auto pt-1">
              {#if space}
                <span
                  class="text-xs px-2 py-0.5 rounded-full"
                  style="background: var(--color-surface-hi); color: var(--color-on-muted);"
                >{space}</span>
              {:else}
                <span></span>
              {/if}

              <!-- Relevance bar -->
              <div class="flex items-center gap-1.5 shrink-0">
                <div
                  class="w-16 h-1.5 rounded-full overflow-hidden"
                  style="background: var(--color-surface-hi);"
                >
                  <div
                    class="h-full rounded-full"
                    style="width: {scorePercent(result.score)}; background: var(--color-primary); opacity: 0.7;"
                  ></div>
                </div>
                <span class="text-xs font-mono" style="color: var(--color-on-muted);">
                  {scorePercent(result.score)}
                </span>
              </div>
            </div>
          </button>
        {/each}
      </div>
    {/if}
  </div>
</div>
