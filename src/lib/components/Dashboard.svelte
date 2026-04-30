<script lang="ts">
  import { spaces, currentSpace, pages, currentPage, currentVersion, versions, userName } from '$lib/stores';
  import { getPages, getPageVersion, listPageVersions, getRecentPages, getAllPresence, getSpaceOverview, updateSpaceOverview, getPageSynthesis, askWiki, lintSpace, createPage, savePageVersion, type RecentPage, type Presence, type SpaceOverview, type Page, type PageSynthesis, type WikiAnswer, type LintResult } from '$lib/api';
  import { FileText, FolderOpen, Clock, ArrowRight, RefreshCw, GitGraph } from 'lucide-svelte';
  import GraphView from './GraphView.svelte';

  let recent = $state<RecentPage[]>([]);
  let loadingRecent = $state(true);
  let allPresence = $state<Presence[]>([]);
  let spaceOverview = $state<SpaceOverview | null>(null);
  let updatingOverview = $state(false);
  let showGraph = $state(false);
  let pageSummaries = $state<Map<string, PageSynthesis>>(new Map());
  let loadingSummaries = $state(false);

  // Graph: use current space if set, else first remote, else any
  function graphSpaceId(): string | null {
    if ($currentSpace) return $currentSpace.id;
    const remote = $spaces.find(s => s.source === 'remote');
    if (remote) return remote.id;
    return $spaces[0]?.id ?? null;
  }

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
          const overview = await getSpaceOverview(s.id);
          if (overview) spaceOverview = overview;
        } catch {}
      }
    }
    loadSynthesis();

    return () => clearInterval(timer);
  });

  $effect(() => {
    const space = $currentSpace;
    if (!space || space.source !== 'remote') { pageSummaries = new Map(); return; }
    const nonEntityPages = $pages.filter(p => !p.is_entity_page);
    if (nonEntityPages.length === 0) return;
    loadingSummaries = true;
    Promise.all(
      nonEntityPages.map(p =>
        getPageSynthesis(space.id, p.id)
          .then(s => s ? [p.id, s] as [string, PageSynthesis] : null)
          .catch(() => null)
      )
    ).then(results => {
      const m = new Map<string, PageSynthesis>();
      for (const r of results) if (r) m.set(r[0], r[1]);
      pageSummaries = m;
    }).finally(() => { loadingSummaries = false; });
  });

  async function navigateToPage(page: Page): Promise<void> {
    $currentPage = page;
    try {
      const ver = await getPageVersion(page.id, $currentSpace!.id);
      if (ver) {
        $currentVersion = ver;
        $versions = await listPageVersions(page.id, $currentSpace!.id);
      }
    } catch {}
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

  let askQuestion = $state('');
  let askLoading = $state(false);
  let askAnswer = $state<WikiAnswer | null>(null);
  let lintLoading = $state(false);
  let lintResult = $state<LintResult | null>(null);
  let savingAnswer = $state(false);

  async function handleAsk() {
    const space = $currentSpace ?? $spaces.find(s => s.source === 'remote');
    if (!space || !askQuestion.trim()) return;
    askLoading = true;
    askAnswer = null;
    try {
      askAnswer = await askWiki(space.id, askQuestion);
    } catch (e) { console.error(e); }
    finally { askLoading = false; }
  }

  async function handleSaveAnswer() {
    const space = $currentSpace ?? $spaces.find(s => s.source === 'remote');
    if (!space || !askAnswer) return;
    savingAnswer = true;
    try {
      const title = askQuestion.slice(0, 60);
      const content = `# ${title}\n\n*Query answer — ${new Date().toISOString().slice(0, 10)}*\n\n${askAnswer.answer}\n\n**Sources:** ${askAnswer.sources.join(', ')}`;
      const page = await createPage(title, space.id, undefined);
      const ver = await getPageVersion(page.id, space.id);
      if (ver) {
        await savePageVersion(ver.id, title, content, content, space.id, ver.updated_at ?? '');
      }
      $pages = [...$pages, page];
      askAnswer = null;
      askQuestion = '';
    } catch (e) { console.error(e); }
    finally { savingAnswer = false; }
  }

  async function handleLint() {
    const space = $currentSpace ?? $spaces.find(s => s.source === 'remote');
    if (!space) return;
    lintLoading = true;
    lintResult = null;
    try {
      lintResult = await lintSpace(space.id);
    } catch (e) { console.error(e); }
    finally { lintLoading = false; }
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

  async function handleGraphPageSelect(pageId: string): Promise<void> {
    // Find which space this page belongs to
    for (const space of $spaces) {
      try {
        const pageList = await getPages(space.id);
        const page = pageList.find(p => p.id === pageId);
        if (page) {
          $currentSpace = space;
          $pages = pageList;
          $currentPage = page;
          $currentVersion = await getPageVersion(page.id, space.id);
          $versions = await listPageVersions(page.id, space.id);
          showGraph = false;
          return;
        }
      } catch {}
    }
  }

  // Top-level spaces (no parent)
  function rootSpaces() {
    return $spaces.filter(s => !s.parent_space_id).slice(0, 6);
  }
</script>

<div class="flex-1 overflow-hidden flex flex-col" style="background: var(--color-panel);">

  <!-- Top bar with graph toggle -->
  <div class="flex items-center justify-end px-8 pt-6 pb-0 flex-shrink-0">
    <button
      onclick={() => { showGraph = !showGraph; }}
      class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors
        {showGraph
          ? 'bg-indigo-600 text-white'
          : ''}" style={!showGraph ? 'color:var(--color-on-muted);border:1px solid var(--color-border);' : ''}
      aria-pressed={showGraph}
    >
      <GitGraph size={13} />
      {showGraph ? 'Graph on' : 'Graph'}
    </button>
  </div>

  <!-- Graph view (full panel) -->
  {#if showGraph}
    {#if graphSpaceId()}
      <div class="flex-1 min-h-0">
        <GraphView
          space_id={graphSpaceId()!}
          onselect={handleGraphPageSelect}
        />
      </div>
    {:else}
      <div class="flex-1 flex items-center justify-center text-sm" style="color:var(--color-on-muted);">
        Create a space first to view the knowledge graph.
      </div>
    {/if}
  {:else}

  <div class="flex-1 overflow-y-auto">
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
      <section class="mb-10 p-5 rounded-xl" style="border:1px solid var(--color-border);background:var(--color-surface);">
        <div class="flex items-center justify-between mb-3">
          <h2 class="text-sm font-semibold" style="color:var(--color-on-surface);">Knowledge Overview</h2>
          <div class="flex items-center gap-2">
            <span class="text-xs" style="color:var(--color-on-muted);">{spaceOverview.synthesized_at.slice(0, 16)}</span>
            <button onclick={handleUpdateOverview} disabled={updatingOverview}
              class="text-xs disabled:opacity-40 flex items-center gap-1" style="color:var(--color-on-muted);">
              <RefreshCw size={11} class={updatingOverview ? 'animate-spin' : ''} />
              {updatingOverview ? 'Updating…' : 'Update'}
            </button>
          </div>
        </div>
        <p class="text-sm leading-relaxed mb-3" style="color:var(--color-on-muted);">{spaceOverview.overview}</p>
        {#if spaceOverview.topics.length > 0}
          <div class="flex flex-wrap gap-1.5">
            {#each spaceOverview.topics as topic}
              <span class="px-2.5 py-0.5 rounded-full text-xs" style="background:var(--color-surface-lo);color:var(--color-on-muted);">{topic}</span>
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
        <h2 class="text-sm font-semibold uppercase tracking-wide mb-3" style="color:var(--color-on-muted);">Who's Active</h2>
        <div class="rounded-lg overflow-hidden" style="border:1px solid var(--color-border);">
          <table class="w-full text-sm">
            <thead>
              <tr style="border-bottom:1px solid var(--color-border);background:var(--color-surface-lo);">
                <th class="text-left px-4 py-2 text-xs font-medium" style="color:var(--color-on-muted);">Person</th>
                <th class="text-left px-4 py-2 text-xs font-medium" style="color:var(--color-on-muted);">Document</th>
                <th class="text-left px-4 py-2 text-xs font-medium" style="color:var(--color-on-muted);">Status</th>
                <th class="text-left px-4 py-2 text-xs font-medium" style="color:var(--color-on-muted);">Last Seen</th>
              </tr>
            </thead>
            <tbody>
              {#each allPresence as p}
                <tr class="transition-colors" style="border-bottom:1px solid var(--color-border);">
                  <td class="px-4 py-2 font-medium" style="color:var(--color-on-surface);">{p.user_name}</td>
                  <td class="px-4 py-2" style="color:var(--color-on-muted);">{p.page_title}</td>
                  <td class="px-4 py-2">
                    <span class="flex items-center gap-1.5 {p.status === 'editing' ? 'text-green-500' : ''}"
                      style={p.status !== 'editing' ? 'color:var(--color-on-muted);' : ''}>
                      <span class="w-1.5 h-1.5 rounded-full inline-block
                        {p.status === 'editing' ? 'bg-green-500' : 'bg-slate-400'}"></span>
                      {p.status}
                    </span>
                  </td>
                  <td class="px-4 py-2 text-xs" style="color:var(--color-on-muted);">{p.last_seen_at}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
    {/if}

    <!-- Pages -->
    {#if $currentSpace?.source === 'remote' && $pages.filter(p => !p.is_entity_page).length > 0}
      <section class="mt-8">
        <h2 style="color: var(--color-on-muted);" class="text-xs font-semibold uppercase tracking-wide mb-3">
          Pages · {$pages.filter(p => !p.is_entity_page).length}
        </h2>
        <div class="flex flex-col gap-2">
          {#each $pages.filter(p => !p.is_entity_page) as page}
            {@const synth = pageSummaries.get(page.id)}
            <button
              onclick={() => navigateToPage(page)}
              class="text-left rounded-xl p-4 transition-colors w-full"
              style="background: var(--color-surface-lo); border: 1px solid var(--color-border);"
              onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface)'}
              onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
            >
              <div class="flex items-start justify-between gap-2">
                <span class="font-medium text-sm" style="color: var(--color-on-surface);">{page.title ?? 'Untitled'}</span>
                {#if synth}
                  <span class="text-xs flex-shrink-0 mt-0.5" style="color: var(--color-on-muted);">{synth.synthesized_at.slice(0,10)}</span>
                {/if}
              </div>
              {#if synth?.summary}
                <p class="text-xs mt-1.5 leading-relaxed line-clamp-2" style="color: var(--color-on-muted);">{synth.summary}</p>
              {:else if loadingSummaries}
                <p class="text-xs mt-1.5" style="color: var(--color-on-muted); opacity: 0.5;">Loading…</p>
              {:else}
                <p class="text-xs mt-1.5" style="color: var(--color-on-muted); opacity: 0.4;">Not yet processed</p>
              {/if}
              {#if synth?.topics && synth.topics.length > 0}
                <div class="flex flex-wrap gap-1 mt-2">
                  {#each synth.topics.slice(0, 4) as topic}
                    <span class="text-xs px-2 py-0.5 rounded-full" style="background: var(--color-surface); color: var(--color-on-muted); border: 1px solid var(--color-border);">{topic}</span>
                  {/each}
                </div>
              {/if}
            </button>
          {/each}
        </div>
      </section>
    {/if}

    <!-- Query -->
    {#if $spaces.some(s => s.source === 'remote')}
      <section class="mt-8">
        <h2 style="color: var(--color-on-muted);" class="text-xs font-semibold uppercase tracking-wide mb-3">Ask</h2>
        <div class="flex gap-2">
          <input
            type="text"
            bind:value={askQuestion}
            placeholder="Ask a question about your knowledge base…"
            onkeydown={(e) => e.key === 'Enter' && handleAsk()}
            class="flex-1 text-sm px-3 py-2 rounded-lg outline-none"
            style="background: var(--color-surface-lo); border: 1px solid var(--color-border); color: var(--color-on-surface);"
          />
          <button
            onclick={handleAsk}
            disabled={askLoading || !askQuestion.trim()}
            class="px-4 py-2 rounded-lg text-sm font-medium text-white disabled:opacity-40"
            style="background: var(--color-primary);"
          >{askLoading ? 'Thinking…' : 'Ask'}</button>
        </div>

        {#if askAnswer}
          <div class="mt-3 rounded-xl p-4 flex flex-col gap-3" style="background: var(--color-surface-lo); border: 1px solid var(--color-border);">
            <div class="flex items-center justify-between">
              <span class="text-xs font-semibold uppercase tracking-wide" style="color: var(--color-on-muted);">Answer</span>
              <div class="flex items-center gap-2">
                <span class="text-xs px-2 py-0.5 rounded-full" style="
                  background: {askAnswer.confidence === 'high' ? '#d1fae5' : askAnswer.confidence === 'medium' ? '#fef3c7' : '#fee2e2'};
                  color: {askAnswer.confidence === 'high' ? '#065f46' : askAnswer.confidence === 'medium' ? '#92400e' : '#991b1b'};">
                  {askAnswer.confidence}
                </span>
                <button
                  onclick={handleSaveAnswer}
                  disabled={savingAnswer}
                  class="text-xs px-2 py-1 rounded-lg"
                  style="color: var(--color-primary); border: 1px solid var(--color-primary);"
                >{savingAnswer ? 'Saving…' : 'Save as page'}</button>
              </div>
            </div>
            <div class="text-sm leading-relaxed whitespace-pre-wrap" style="color: var(--color-on-surface);">{askAnswer.answer}</div>
            {#if askAnswer.sources.length > 0}
              <p class="text-xs" style="color: var(--color-on-muted);">Sources: {askAnswer.sources.slice(0, 5).join(' · ')}</p>
            {/if}
          </div>
        {/if}
      </section>
    {/if}

    <!-- Lint -->
    {#if $spaces.some(s => s.source === 'remote')}
      <section class="mt-8">
        <div class="flex items-center justify-between mb-3">
          <h2 style="color: var(--color-on-muted);" class="text-xs font-semibold uppercase tracking-wide">Lint</h2>
          <button
            onclick={handleLint}
            disabled={lintLoading}
            class="text-xs px-3 py-1 rounded-lg"
            style="color: var(--color-on-muted); border: 1px solid var(--color-border);"
          >{lintLoading ? 'Checking…' : 'Run health check'}</button>
        </div>

        {#if lintResult}
          <div class="flex flex-col gap-3">
            {#if lintResult.orphan_wiki_pages.length > 0}
              <div class="rounded-lg p-3" style="background: var(--color-surface-lo); border: 1px solid var(--color-border);">
                <p class="text-xs font-semibold mb-1" style="color: #b45309;">Orphan pages ({lintResult.orphan_wiki_pages.length})</p>
                <p class="text-xs" style="color: var(--color-on-muted);">{lintResult.orphan_wiki_pages.join(', ')}</p>
              </div>
            {/if}
            {#if lintResult.stale_wiki_pages.length > 0}
              <div class="rounded-lg p-3" style="background: var(--color-surface-lo); border: 1px solid var(--color-border);">
                <p class="text-xs font-semibold mb-1" style="color: #b45309;">Stale pages ({lintResult.stale_wiki_pages.length})</p>
                <p class="text-xs" style="color: var(--color-on-muted);">{lintResult.stale_wiki_pages.join(', ')}</p>
              </div>
            {/if}
            {#if lintResult.high_mention_unlinked.length > 0}
              <div class="rounded-lg p-3" style="background: var(--color-surface-lo); border: 1px solid var(--color-border);">
                <p class="text-xs font-semibold mb-1" style="color: var(--color-primary);">High-mention entities without wiki pages</p>
                <p class="text-xs" style="color: var(--color-on-muted);">{lintResult.high_mention_unlinked.join(', ')}</p>
              </div>
            {/if}
            {#if lintResult.investigation_questions.length > 0}
              <div class="rounded-lg p-3" style="background: var(--color-surface-lo); border: 1px solid var(--color-border);">
                <p class="text-xs font-semibold mb-2" style="color: var(--color-on-surface);">Questions to investigate</p>
                <ul class="space-y-1">
                  {#each lintResult.investigation_questions as q}
                    <li class="text-xs flex gap-2" style="color: var(--color-on-muted);">
                      <span style="color: var(--color-primary);">→</span>
                      <button
                        class="text-left hover:underline"
                        style="color: var(--color-on-muted);"
                        onclick={() => { askQuestion = q; }}
                      >{q}</button>
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}
            {#if lintResult.suggested_sources.length > 0}
              <div class="rounded-lg p-3" style="background: var(--color-surface-lo); border: 1px solid var(--color-border);">
                <p class="text-xs font-semibold mb-2" style="color: var(--color-on-surface);">Suggested sources to add</p>
                <ul class="space-y-1">
                  {#each lintResult.suggested_sources as s}
                    <li class="text-xs" style="color: var(--color-on-muted);">· {s}</li>
                  {/each}
                </ul>
              </div>
            {/if}
          </div>
        {/if}
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

  {/if}
</div>
