<script lang="ts">
  import { spaces, currentSpace, pages, currentPage, currentVersion, versions, userName } from '$lib/stores';
  import { getPages, getPageVersion, listPageVersions, getRecentPages, getAllPresence, getEntitySuggestions, promoteEntity, dismissEntity, getSpaceOverview, updateSpaceOverview, type RecentPage, type Presence, type EntitySuggestion, type SpaceOverview } from '$lib/api';
  import { FileText, FolderOpen, Clock, ArrowRight, RefreshCw } from 'lucide-svelte';

  let recent = $state<RecentPage[]>([]);
  let loadingRecent = $state(true);
  let allPresence = $state<Presence[]>([]);
  let entitySuggestions = $state<EntitySuggestion[]>([]);
  let spaceOverview = $state<SpaceOverview | null>(null);
  let promotingId = $state<string | null>(null);
  let updatingOverview = $state(false);

  // Load recent pages reactively — fires once spaces are populated (after DB init).
  // Using $effect instead of onMount because Dashboard's onMount runs before layout's
  // onMount which initializes the registry DB.
  $effect(() => {
    if ($spaces.length === 0) return;
    if (!loadingRecent) return;
    getRecentPages(8).then(r => { recent = r; }).catch(() => { recent = []; }).finally(() => { loadingRecent = false; });
  });

  $effect(() => {
    // Load presence from all remote spaces
    const remoteSpaces = $spaces.filter(s => s.source === 'remote');
    if (remoteSpaces.length === 0) return;

    async function loadPresence() {
      const results: Presence[] = [];
      for (const s of remoteSpaces) {
        try {
          const p = await getAllPresence(s.id);
          results.push(...p);
        } catch {}
      }
      allPresence = results;
    }

    loadPresence();
    const timer = setInterval(loadPresence, 15_000);

    async function loadSynthesis() {
      for (const s of remoteSpaces) {
        try {
          const [entities, overview] = await Promise.all([
            getEntitySuggestions(s.id),
            getSpaceOverview(s.id),
          ]);
          if (entities.length > 0) entitySuggestions = entities;
          if (overview) spaceOverview = overview;
        } catch {}
      }
    }
    loadSynthesis();

    return () => clearInterval(timer);
  });

  async function handlePromoteEntity(entityId: string) {
    const space = $spaces.find(s => s.source === 'remote');
    if (!space) return;
    promotingId = entityId;
    try {
      await promoteEntity(space.id, entityId);
      entitySuggestions = await getEntitySuggestions(space.id);
    } catch (e) { console.error(e); }
    finally { promotingId = null; }
  }

  async function handleDismissEntity(entityId: string) {
    const space = $spaces.find(s => s.source === 'remote');
    if (!space) return;
    try {
      await dismissEntity(space.id, entityId);
      entitySuggestions = entitySuggestions.filter(e => e.id !== entityId);
    } catch (e) { console.error(e); }
  }

  async function handleUpdateOverview() {
    const space = $spaces.find(s => s.source === 'remote');
    if (!space) return;
    updatingOverview = true;
    try {
      spaceOverview = await updateSpaceOverview(space.id);
    } catch (e) { console.error(e); }
    finally { updatingOverview = false; }
  }

  function greeting(): string {
    const h = new Date().getHours();
    if (h < 12) return 'Good morning';
    if (h < 17) return 'Good afternoon';
    return 'Good evening';
  }

  function relativeTime(iso: string): string {
    const diff = Date.now() - new Date(iso).getTime();
    const m = Math.floor(diff / 60000);
    if (m < 1) return 'just now';
    if (m < 60) return `${m}m ago`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h ago`;
    return `${Math.floor(h / 24)}d ago`;
  }

  async function openRecent(r: RecentPage) {
    // Find the space first
    const space = $spaces.find(s => s.id === r.space_id);
    if (!space) return;
    $currentSpace = space;
    const pageList = await getPages(space.id);
    // Merge into pages store so sidebar tree shows correctly
    $pages = pageList;
    const page = pageList.find(p => p.id === r.id);
    if (!page) return;
    $currentPage = page;
    $currentVersion = await getPageVersion(page.id, space.id);
    $versions = await listPageVersions(page.id, space.id);
  }

  // Top-level spaces (no parent)
  function rootSpaces() {
    return $spaces.filter(s => !s.parent_space_id).slice(0, 6);
  }
</script>

<div class="flex-1 overflow-y-auto" style="background: var(--color-panel);">
  <div class="max-w-4xl mx-auto px-8 py-10">

    <!-- Greeting header -->
    <div class="mb-10">
      <h1 class="text-3xl font-bold text-on-surface mb-1">
        {greeting()}, {$userName || 'there'} 👋
      </h1>
      <p class="text-on-muted text-sm">Here's what's happening in your workspace.</p>
    </div>

    <!-- Space Overview -->
    {#if spaceOverview}
      <section class="mb-10 p-5 rounded-xl border border-slate-800 bg-slate-900/40">
        <div class="flex items-center justify-between mb-3">
          <h2 class="text-sm font-semibold text-slate-300">Knowledge Overview</h2>
          <div class="flex items-center gap-2">
            <span class="text-xs text-slate-600">{spaceOverview.synthesized_at.slice(0, 16)}</span>
            <button onclick={handleUpdateOverview} disabled={updatingOverview}
              class="text-xs text-slate-500 hover:text-slate-300 disabled:opacity-40 flex items-center gap-1">
              <RefreshCw size={11} class={updatingOverview ? 'animate-spin' : ''} />
              {updatingOverview ? 'Updating…' : 'Update'}
            </button>
          </div>
        </div>
        <p class="text-slate-400 text-sm leading-relaxed mb-3">{spaceOverview.overview}</p>
        {#if spaceOverview.topics.length > 0}
          <div class="flex flex-wrap gap-1.5">
            {#each spaceOverview.topics as topic}
              <span class="px-2.5 py-0.5 rounded-full bg-slate-800 text-slate-400 text-xs">{topic}</span>
            {/each}
          </div>
        {/if}
      </section>
    {/if}

    <!-- Recent pages -->
    <section class="mb-10">
      <div class="flex items-center justify-between mb-4">
        <h2 class="text-sm font-semibold text-on-muted uppercase tracking-widest">Recent Pages</h2>
      </div>

      {#if loadingRecent}
        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
          {#each [1,2,3] as _}
            <div class="rounded-xl p-4 animate-pulse" style="background: var(--color-surface-lo); height: 88px;"></div>
          {/each}
        </div>
      {:else if recent.length === 0}
        <div class="rounded-xl p-8 text-center" style="background: var(--color-surface); border: 1px dashed var(--color-border);">
          <Clock size={28} class="mx-auto mb-2" style="color: var(--color-on-muted); opacity: 0.4;" />
          <p class="text-on-muted text-sm">No recent pages yet — open a page to see it here.</p>
        </div>
      {:else}
        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
          {#each recent as r (r.id)}
            <button
              onclick={() => openRecent(r)}
              class="group text-left rounded-xl p-4 transition-all hover:-translate-y-0.5"
              style="background: var(--color-surface); box-shadow: var(--shadow-card);"
            >
              <div class="flex items-start gap-3">
                <div class="w-8 h-8 rounded-lg flex items-center justify-center flex-shrink-0"
                  style="background: var(--color-surface-lo);">
                  <FileText size={14} style="color: var(--color-primary);" />
                </div>
                <div class="min-w-0 flex-1">
                  <p class="text-sm font-semibold text-on-surface truncate group-hover:text-primary transition-colors">{r.title}</p>
                  <p class="text-xs text-on-muted mt-0.5 truncate">{r.space_name}</p>
                  <p class="text-xs mt-1" style="color: var(--color-on-muted); opacity: 0.6;">{relativeTime(r.last_accessed_at)}</p>
                </div>
              </div>
            </button>
          {/each}
        </div>
      {/if}
    </section>

    <!-- Who's Active -->
    {#if allPresence.length > 0}
      <section class="mt-8">
        <h2 class="text-sm font-semibold text-slate-400 uppercase tracking-wide mb-3">Who's Active</h2>
        <div class="rounded-lg border border-slate-800 overflow-hidden">
          <table class="w-full text-sm">
            <thead>
              <tr class="border-b border-slate-800 bg-slate-900/50">
                <th class="text-left px-4 py-2 text-xs text-slate-500 font-medium">Person</th>
                <th class="text-left px-4 py-2 text-xs text-slate-500 font-medium">Document</th>
                <th class="text-left px-4 py-2 text-xs text-slate-500 font-medium">Status</th>
                <th class="text-left px-4 py-2 text-xs text-slate-500 font-medium">Last Seen</th>
              </tr>
            </thead>
            <tbody>
              {#each allPresence as p}
                <tr class="border-b border-slate-800/50 hover:bg-slate-800/30 transition-colors">
                  <td class="px-4 py-2 text-slate-200">{p.user_name}</td>
                  <td class="px-4 py-2 text-slate-300">{p.page_title}</td>
                  <td class="px-4 py-2">
                    <span class="flex items-center gap-1.5
                      {p.status === 'editing' ? 'text-green-400' : 'text-slate-400'}">
                      <span class="w-1.5 h-1.5 rounded-full inline-block
                        {p.status === 'editing' ? 'bg-green-400' : 'bg-slate-500'}"></span>
                      {p.status}
                    </span>
                  </td>
                  <td class="px-4 py-2 text-slate-500 text-xs">{p.last_seen_at}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
    {/if}

    <!-- Entity Suggestions -->
    {#if entitySuggestions.length > 0}
      <section class="mt-8 mb-10">
        <div class="flex items-center justify-between mb-3">
          <h2 class="text-sm font-semibold text-on-muted uppercase tracking-widest">Entity Suggestions</h2>
          <span class="text-xs text-slate-500">LLM-discovered across your docs</span>
        </div>
        <div class="rounded-xl border border-slate-800 overflow-hidden">
          <table class="w-full text-sm">
            <thead>
              <tr class="border-b border-slate-800 bg-slate-900/50">
                <th class="text-left px-4 py-2 text-xs text-slate-500 font-medium">Entity</th>
                <th class="text-left px-4 py-2 text-xs text-slate-500 font-medium">Type</th>
                <th class="text-left px-4 py-2 text-xs text-slate-500 font-medium">Mentions</th>
                <th class="text-left px-4 py-2 text-xs text-slate-500 font-medium">Description</th>
                <th class="px-4 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {#each entitySuggestions as entity (entity.id)}
                <tr class="border-b border-slate-800/50 hover:bg-slate-800/20 transition-colors">
                  <td class="px-4 py-2.5 text-slate-200 font-medium">{entity.name}</td>
                  <td class="px-4 py-2.5">
                    <span class="px-2 py-0.5 rounded-full text-xs
                      {entity.entity_type === 'person' ? 'bg-blue-900/50 text-blue-300' :
                       entity.entity_type === 'project' ? 'bg-purple-900/50 text-purple-300' :
                       entity.entity_type === 'decision' ? 'bg-amber-900/50 text-amber-300' :
                       'bg-slate-800 text-slate-400'}">
                      {entity.entity_type}
                    </span>
                  </td>
                  <td class="px-4 py-2.5 text-slate-400">{entity.mention_count}</td>
                  <td class="px-4 py-2.5 text-slate-500 text-xs max-w-xs truncate">{entity.description}</td>
                  <td class="px-4 py-2.5">
                    <div class="flex gap-2 justify-end">
                      {#if entity.status === 'candidate'}
                        <button onclick={() => handlePromoteEntity(entity.id)}
                          disabled={promotingId === entity.id}
                          class="px-2 py-1 rounded text-xs bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-50">
                          {promotingId === entity.id ? '…' : 'Promote'}
                        </button>
                        <button onclick={() => handleDismissEntity(entity.id)}
                          class="px-2 py-1 rounded text-xs text-slate-500 hover:text-slate-300">
                          Dismiss
                        </button>
                      {:else}
                        <span class="text-xs text-indigo-400">promoted</span>
                      {/if}
                    </div>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
    {/if}

    <!-- Spaces overview -->
    {#if $spaces.length > 0}
      <section>
        <div class="flex items-center justify-between mb-4">
          <h2 class="text-sm font-semibold text-on-muted uppercase tracking-widest">Spaces</h2>
          <span class="text-xs text-on-muted">{$spaces.length} total</span>
        </div>
        <div class="grid grid-cols-2 sm:grid-cols-3 gap-3">
          {#each rootSpaces() as space (space.id)}
            <button
              onclick={async () => {
                const { currentSpace: cs, pages: pg } = await import('$lib/stores');
                const { getPages: gp } = await import('$lib/api');
                // set currentSpace via store
                currentSpace.set(space);
                pages.set(await getPages(space.id));
              }}
              class="group text-left rounded-xl p-4 transition-all hover:-translate-y-0.5"
              style="background: var(--color-surface); box-shadow: var(--shadow-card);"
            >
              <div class="w-9 h-9 rounded-xl flex items-center justify-center mb-3"
                style="background: linear-gradient(135deg, var(--color-primary), var(--color-primary-dim));">
                <FolderOpen size={16} color="white" />
              </div>
              <p class="text-sm font-semibold text-on-surface truncate group-hover:text-primary transition-colors">{space.name}</p>
              {#if space.description}
                <p class="text-xs text-on-muted mt-0.5 truncate">{space.description}</p>
              {/if}
            </button>
          {/each}
        </div>
      </section>
    {:else}
      <section>
        <div class="rounded-xl p-10 text-center" style="background: var(--color-surface); box-shadow: var(--shadow-ambient);">
          <div class="w-14 h-14 rounded-2xl flex items-center justify-center mx-auto mb-4"
            style="background: linear-gradient(135deg, var(--color-primary), var(--color-primary-dim));">
            <FolderOpen size={24} color="white" />
          </div>
          <h3 class="text-base font-bold text-on-surface mb-1">Welcome to Bamako</h3>
          <p class="text-sm text-on-muted">Create your first space in the sidebar to get started.</p>
        </div>
      </section>
    {/if}

  </div>
</div>
