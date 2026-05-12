<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Crepe } from '@milkdown/crepe';
  import { replaceAll } from '@milkdown/kit/utils';
  import { listenerCtx } from '@milkdown/kit/plugin/listener';
  import '@milkdown/crepe/theme/common/style.css';
  import '@milkdown/crepe/theme/frame-dark.css';
  import mermaid from 'mermaid';
  import { PanelRight } from 'lucide-svelte';
  import { currentVersion, currentPage, currentSpace, readMode, theme, userName, lastSynthesisAt, activityStart, activityDone, activityError, pages, versions } from '$lib/stores';
  import { savePageVersion, type SaveResult, upsertPresence, clearPresence, getPagePresence, type Presence, getPageSynthesis, getPageLinks, synthesizePage, type PageSynthesis, type PageLink, getPageImage, getPageVersion, listPageVersions } from '$lib/api';

  async function inlineImages(md: string): Promise<string> {
    // Convert [[Title]] to markdown links before Milkdown parses — brackets get stripped otherwise
    let out = md.replace(/\[\[([^\]]+)\]\]/g, (_, title) => `[${title}](#wiki:${encodeURIComponent(title)})`);
    const refs = [...out.matchAll(/spaceimg:\/\/([^\s)]+)/g)];
    if (refs.length === 0) return out;
    const replacements = await Promise.all(
      refs.map(async ([full, id]) => {
        try { return [full, await getPageImage(id)] as [string, string]; }
        catch { return [full, full] as [string, string]; }
      })
    );
    for (const [orig, dataUri] of replacements) out = out.replaceAll(orig, dataUri);
    return out;
  }

  async function navigateToLinkedPage(title: string): Promise<void> {
    const space = $currentSpace;
    if (!space) return;
    const target = $pages.find(p => p.title === title);
    if (!target) return;
    $currentPage = target;
    try {
      const ver = await getPageVersion(target.id, space.id);
      if (ver) {
        $currentVersion = ver;
        $versions = await listPageVersions(target.id, space.id);
      }
    } catch {}
  }

  function renderWikiLinks(): void {
    // no-op: wiki links are now pre-converted to #wiki: markdown links in inlineImages()
    // click handling is via the container click listener set up in onMount
  }

  let container: HTMLDivElement;
  let crepe: Crepe;
  let mode = $state<'wysiwyg' | 'source'>('wysiwyg');
  let markdownSource = $state('');
  let saveTimer: ReturnType<typeof setTimeout>;
  let mermaidTimer: ReturnType<typeof setTimeout>;
  let pagePresence = $state<Presence[]>([]);
  let presenceTimer: ReturnType<typeof setInterval>;
  let loadedVersionId: string | null = null;
  let ready = $state(false);
  let baseUpdatedAt = $state<string>('');
  let conflict = $state<(SaveResult & { type: 'conflict' }) | null>(null);
  let myContent = $state('');
  let showInsights = $state(true);
  let synthesis = $state<PageSynthesis | null>(null);
  let pageLinks = $state<PageLink[]>([]);
  let synthesizing = $state(false);
  $effect(() => {
    const page = $currentPage;
    const space = $currentSpace;
    const _tick = $lastSynthesisAt;
    if (!page || !space) { synthesis = null; pageLinks = []; return; }
    getPageSynthesis(space.id, page.id).then(s => { synthesis = s; }).catch(() => {});
    getPageLinks(space.id, page.id).then(l => { pageLinks = l; }).catch(() => {});
  });

  mermaid.initialize({ startOnLoad: false, theme: 'dark' });

  onMount(async () => {
    const initialMd = await inlineImages($currentVersion?.content ?? '');
    crepe = new Crepe({
      root: container,
      defaultValue: initialMd,
    });

    crepe.on((listener) => {
      listener.markdownUpdated((_, markdown) => {
        if (!$readMode) scheduleAutoSave(markdown);
        scheduleMermaid();
      });
    });

    await crepe.create();
    loadedVersionId = $currentVersion?.id ?? null;
    ready = true;
    scheduleMermaid();

    // Intercept clicks on wiki links (converted from [[Title]] to #wiki: hrefs)
    container.addEventListener('click', (e) => {
      const link = (e.target as HTMLElement).closest('a[href^="#wiki:"]');
      if (link) {
        e.preventDefault();
        const title = decodeURIComponent(link.getAttribute('href')!.slice(6));
        navigateToLinkedPage(title);
      }
    }, true);

    // Initial presence upsert
    const space = $currentSpace;
    const page = $currentPage;
    const version = $currentVersion;
    if (space?.source === 'remote' && page && $userName) {
      upsertPresence(space.id, page.id, version?.title ?? page.title ?? 'Untitled', $userName, 'viewing').catch(() => {});
      // Heartbeat + poll every 20s — re-upsert keeps last_seen_at fresh so we don't expire
      presenceTimer = setInterval(async () => {
        const sp = $currentSpace;
        const pg = $currentPage;
        if (sp?.source === 'remote' && pg && $userName) {
          try {
            await upsertPresence(sp.id, pg.id, $currentVersion?.title ?? pg.title ?? 'Untitled', $userName, 'viewing');
            pagePresence = await getPagePresence(sp.id, pg.id);
          } catch {}
        }
      }, 20_000);
      // Initial fetch
      try {
        pagePresence = await getPagePresence(space.id, page.id);
      } catch {}
    }
  });

  onDestroy(() => {
    crepe?.destroy();
    clearInterval(presenceTimer);
    const space = $currentSpace;
    const page = $currentPage;
    if (space?.source === 'remote' && page && $userName) {
      clearPresence(space.id, page.id, $userName).catch(() => {});
    }
  });

  // Reload content when version changes
  $effect(() => {
    const ver = $currentVersion;
    if (!ver || !ready || ver.id === loadedVersionId) return;
    loadedVersionId = ver.id;
    baseUpdatedAt = ver.updated_at ?? '';
    const md = ver.content ?? '';
    if (mode === 'wysiwyg') {
      inlineImages(md).then(processed => {
        crepe.editor.action(replaceAll(processed));
        scheduleMermaid();
      });
    } else {
      markdownSource = md;
    }
  });

  // Update mermaid theme when app theme changes
  $effect(() => {
    const t = $theme;
    mermaid.initialize({ startOnLoad: false, theme: t === 'light' ? 'default' : 'dark' });
  });

  function scheduleAutoSave(markdown: string) {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
      if (!$currentVersion) return;
      try {
        const result = await savePageVersion(
          $currentVersion.id,
          $currentVersion.title ?? 'Untitled',
          markdown,
          markdown,
          $currentSpace?.id ?? $currentPage?.space_id ?? '',
          baseUpdatedAt
        );
        if (result.type === 'conflict') {
          myContent = markdown;
          conflict = result;
        } else {
          baseUpdatedAt = result.new_updated_at;
          // Mark as editing on save
          const space = $currentSpace;
          const page = $currentPage;
          if (space?.source === 'remote' && page && $userName) {
            upsertPresence(space.id, page.id, $currentVersion?.title ?? 'Untitled', $userName, 'editing').catch(() => {});
          }
        }
      } catch (e) {
        console.error('auto-save failed:', e);
      }
    }, 800);
  }

  async function keepMine() {
    if (!conflict || !$currentVersion) return;
    try {
      const result = await savePageVersion(
        $currentVersion.id,
        $currentVersion.title ?? 'Untitled',
        myContent,
        myContent,
        $currentSpace?.id ?? $currentPage?.space_id ?? '',
        conflict.current_updated_at
      );
      if (result.type === 'ok') {
        baseUpdatedAt = result.new_updated_at;
      }
    } catch (e) {
      console.error('force-save failed:', e);
    }
    conflict = null;
  }

  function keepTheirs() {
    if (!conflict || !$currentVersion) return;
    baseUpdatedAt = conflict.current_updated_at;
    if (mode === 'wysiwyg') {
      crepe.editor.action(replaceAll(conflict.current_content));
    } else {
      markdownSource = conflict.current_content;
    }
    conflict = null;
  }

  function scheduleMermaid() {
    clearTimeout(mermaidTimer);
    mermaidTimer = setTimeout(renderMermaid, 600);
  }

  async function renderMermaid() {
    if (!container) return;
    const blocks = container.querySelectorAll('pre code.language-mermaid');
    for (const codeEl of blocks) {
      const pre = codeEl.parentElement;
      if (!pre || pre.dataset.mermaidDone) continue;
      pre.dataset.mermaidDone = 'true';
      try {
        const id = `mmd-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
        const { svg } = await mermaid.render(id, codeEl.textContent ?? '');
        const wrapper = document.createElement('div');
        wrapper.className = 'mermaid-output';
        wrapper.style.cssText = 'margin: 1rem 0; text-align: center;';
        wrapper.innerHTML = svg;
        pre.insertAdjacentElement('afterend', wrapper);
        pre.style.display = 'none';
      } catch (e) {
        console.error('mermaid render failed:', e);
      }
    }
    renderWikiLinks();
  }

  function toggleMode() {
    if (!ready) return;
    if (mode === 'wysiwyg') {
      markdownSource = crepe.getMarkdown();
      mode = 'source';
    } else {
      mode = 'wysiwyg';
      setTimeout(() => {
        crepe.editor.action(replaceAll(markdownSource));
        scheduleAutoSave(markdownSource);
        scheduleMermaid();
      }, 50);
    }
  }
</script>

<div class="flex flex-row flex-1 min-h-0" data-editor-bg style="background: var(--color-surface, #1e1e2e);">
  <!-- Main editor column -->
  <div class="flex flex-col flex-1 min-w-0 min-h-0">

  <!-- Mode toggle + read indicator -->
  <div class="flex items-center justify-between px-4 py-1 border-b flex-shrink-0 {$readMode ? 'bg-amber-900/10' : ''}"
    style="border-color: var(--color-border);">
    {#if !$readMode}
      <div class="flex rounded overflow-hidden border text-xs" style="border-color: var(--color-border);">
        <button
          onclick={() => mode !== 'wysiwyg' && toggleMode()}
          class="px-3 py-1 transition-colors"
          style={mode === 'wysiwyg'
            ? 'background: var(--color-primary); color: #fff;'
            : 'color: var(--color-on-muted);'}
        >Rich</button>
        <button
          onclick={() => mode !== 'source' && toggleMode()}
          class="px-3 py-1 transition-colors"
          style={mode === 'source'
            ? 'background: var(--color-primary); color: #fff;'
            : 'color: var(--color-on-muted);'}
        >Markdown</button>
      </div>
    {:else}
      <span class="text-xs text-amber-500">Read only — click <strong class="text-amber-400">Edit</strong> in the toolbar to make changes</span>
    {/if}
    <button
      onclick={() => showInsights = !showInsights}
      class="ml-2 flex items-center gap-1 px-2 py-1.5 rounded transition-colors text-xs"
      title="Toggle Insights"
      style={showInsights
        ? 'background: var(--color-primary); color: #fff;'
        : 'color: var(--color-on-muted); border: 1px solid var(--color-border);'}
    >
      <PanelRight size={13} />
      {#if !showInsights}Insights{/if}
    </button>
  </div>

  {#if pagePresence.filter(p => p.user_name !== $userName).length > 0}
    <div class="flex items-center gap-2 px-4 py-1 border-b flex-shrink-0"
      style="border-color: var(--color-border); background: var(--color-surface-lo);">
      <span class="text-xs" style="color: var(--color-on-muted);">Also here:</span>
      {#each pagePresence.filter(p => p.user_name !== $userName) as p}
        <span class="flex items-center gap-1 text-xs px-2 py-0.5 rounded-full
          {p.status === 'editing' ? 'bg-green-900/50 text-green-300' : ''}">
          {#if p.status !== 'editing'}
            <span class="w-1.5 h-1.5 rounded-full inline-block" style="background: var(--color-on-muted);"></span>
          {:else}
            <span class="w-1.5 h-1.5 rounded-full bg-green-400 inline-block"></span>
          {/if}
          {p.user_name}
          <span class="opacity-60">{p.status === 'editing' ? 'editing' : 'viewing'}</span>
        </span>
      {/each}
    </div>
  {/if}

  <!-- WYSIWYG: Milkdown/Crepe -->
  <div
    bind:this={container}
    class="flex-1 overflow-y-auto relative"
    style:pointer-events={$readMode ? 'none' : 'auto'}
    style:display={mode === 'wysiwyg' ? 'block' : 'none'}
  ></div>

  <!-- Source: editable markdown textarea -->
  {#if mode === 'source'}
    <textarea
      class="flex-1 font-mono text-sm p-8 resize-none outline-none border-none leading-relaxed"
      style="background: transparent; color: var(--color-on-surface);"
      bind:value={markdownSource}
      oninput={() => scheduleAutoSave(markdownSource)}
      readonly={$readMode}
      placeholder="Write markdown here..."
      spellcheck={false}
    ></textarea>
  {/if}

  {#if conflict}
    <div class="fixed inset-0 z-50 flex flex-col p-6 gap-4" style="background: var(--color-surface); opacity: 0.97;">
      <div class="text-amber-400 font-semibold text-sm">
        ⚠ Conflict — this document was edited by someone else while you were writing
      </div>
      <div class="flex flex-1 gap-4 min-h-0 overflow-hidden">
        <div class="flex-1 flex flex-col gap-2 min-h-0">
          <div class="text-xs font-semibold uppercase tracking-wide" style="color: var(--color-on-muted);">Your version</div>
          <textarea readonly class="flex-1 font-mono text-sm p-4 rounded resize-none overflow-y-auto"
            style="background: var(--color-surface-lo); color: var(--color-on-surface);"
            value={myContent}></textarea>
        </div>
        <div class="flex-1 flex flex-col gap-2 min-h-0">
          <div class="text-xs font-semibold uppercase tracking-wide" style="color: var(--color-on-muted);">Their version (server)</div>
          <textarea readonly class="flex-1 font-mono text-sm p-4 rounded resize-none overflow-y-auto"
            style="background: var(--color-surface-lo); color: var(--color-on-surface);"
            value={conflict.current_content}></textarea>
        </div>
      </div>
      <div class="flex gap-3 justify-end">
        <button onclick={keepTheirs} class="px-4 py-2 rounded text-sm"
          style="background: var(--color-surface-lo); color: var(--color-on-surface);">Keep Theirs</button>
        <button onclick={keepMine} class="px-4 py-2 rounded text-sm text-white"
          style="background: var(--color-primary);">Keep Mine (overwrite)</button>
      </div>
    </div>
  {/if}

  </div><!-- end main editor column -->

  <!-- Insights panel -->
  {#if showInsights}
    <aside class="w-80 flex flex-col overflow-y-auto flex-shrink-0 text-sm"
      style="border-left: 1px solid var(--color-border); background: var(--color-surface-lo);">

      <!-- Panel header -->
      <div class="flex items-center justify-between px-4 py-3 flex-shrink-0"
        style="border-bottom: 1px solid var(--color-border);">
        <span class="text-xs font-semibold uppercase tracking-wide" style="color: var(--color-on-muted);">Insights</span>
        <button
          onclick={() => showInsights = false}
          class="w-6 h-6 flex items-center justify-center rounded text-xs transition-opacity opacity-60 hover:opacity-100"
          style="color: var(--color-on-surface);"
          title="Close insights"
        >×</button>
      </div>

      <div class="flex flex-col gap-4 p-4">
        {#if $currentPage?.is_entity_page === 1}
          <!-- Wiki page metadata -->
          <section>
            <div class="flex items-center gap-2 mb-3">
              <span class="text-xs px-2 py-0.5 rounded-full font-medium" style="background: var(--color-surface); border: 1px solid var(--color-border); color: var(--color-primary);">Wiki Page</span>
            </div>
            <p class="text-xs leading-relaxed" style="color: var(--color-on-muted);">Auto-generated from extracted entities. Use Demote in the toolbar to remove.</p>
          </section>

          {#if pageLinks.length > 0}
            <section>
              <h3 class="text-xs font-semibold uppercase tracking-wide mb-2" style="color: var(--color-on-muted);">Backlinks</h3>
              <div class="flex flex-col gap-1.5">
                {#each pageLinks as link}
                  <button
                    onclick={() => navigateToLinkedPage(link.other_page_title)}
                    class="text-left text-xs px-2.5 py-2 rounded-lg transition-colors w-full"
                    style="background: var(--color-surface-lo); border: 1px solid var(--color-border); color: var(--color-on-surface);"
                    onmouseenter={(e) => (e.currentTarget as HTMLElement).style.borderColor = 'var(--color-primary)'}
                    onmouseleave={(e) => (e.currentTarget as HTMLElement).style.borderColor = 'var(--color-border)'}
                  >
                    <div class="font-medium">{link.other_page_title}</div>
                    {#if link.relationship && link.relationship !== 'mentions'}
                      <div class="mt-0.5 opacity-60">{link.relationship}</div>
                    {/if}
                  </button>
                {/each}
              </div>
            </section>
          {/if}

          {#if $currentPage?.created_at}
            <p class="text-xs mt-auto pt-2" style="color: var(--color-on-muted); border-top: 1px solid var(--color-border);">Created {$currentPage.created_at.slice(0, 10)}</p>
          {/if}

        {:else}
          <!-- Normal page synthesis content -->
          {#if !synthesis}
            <p class="text-xs leading-relaxed" style="color: var(--color-on-muted);">No synthesis data for this page yet.</p>
              <button
                onclick={async () => {
                  const space = $currentSpace;
                  const page = $currentPage;
                  if (!space || !page) return;
                  synthesizing = true;
                  const id = activityStart(`Synthesizing: ${page.title ?? 'page'}…`);
                  try {
                    const result = await synthesizePage(space.id, page.id);
                    synthesis = result;
                    activityDone(id, `Synthesized: ${page.title ?? 'page'}`);
                  } catch (e) {
                    activityError(id, `Synthesis failed: ${e}`);
                  } finally {
                    synthesizing = false;
                  }
                }}
                disabled={synthesizing}
                class="px-3 py-1.5 rounded text-xs text-white disabled:opacity-50 flex items-center gap-1.5"
                style="background: var(--color-primary);"
              >
                {#if synthesizing}
                  <span class="w-3 h-3 border border-white border-t-transparent rounded-full animate-spin inline-block"></span>
                  Synthesizing…
                {:else}
                  ✦ Synthesize this page
                {/if}
              </button>
          {:else}
            <!-- Summary -->
            <section>
              <h3 class="text-xs font-semibold uppercase tracking-wide mb-1.5" style="color: var(--color-on-muted);">Summary</h3>
              <p class="text-xs leading-relaxed" style="color: var(--color-on-surface);">{synthesis.summary}</p>
            </section>

            <!-- Key Points -->
            {#if synthesis.key_points.length > 0}
              <section>
                <h3 class="text-xs font-semibold uppercase tracking-wide mb-1.5" style="color: var(--color-on-muted);">Key Points</h3>
                <ul class="space-y-1.5">
                  {#each synthesis.key_points as point}
                    <li class="text-xs flex gap-1.5 leading-relaxed" style="color: var(--color-on-surface);">
                      <span class="flex-shrink-0 mt-0.5" style="color: var(--color-primary);">•</span>{point}
                    </li>
                  {/each}
                </ul>
              </section>
            {/if}

            <!-- Topics -->
            {#if synthesis.topics.length > 0}
              <section>
                <h3 class="text-xs font-semibold uppercase tracking-wide mb-1.5" style="color: var(--color-on-muted);">Topics</h3>
                <div class="flex flex-wrap gap-1">
                  {#each synthesis.topics as topic}
                    <span class="px-2 py-0.5 rounded-full text-xs"
                      style="background: var(--color-surface); color: var(--color-on-muted); border: 1px solid var(--color-border);"
                    >{topic}</span>
                  {/each}
                </div>
              </section>
            {/if}

<!-- Related Pages -->
            {#if pageLinks.length > 0}
              <section>
                <h3 class="text-xs font-semibold uppercase tracking-wide mb-2" style="color: var(--color-on-muted);">Related</h3>
                <div class="flex flex-col gap-1.5">
                  {#each pageLinks as link}
                    <button
                      onclick={() => navigateToLinkedPage(link.other_page_title)}
                      class="text-left text-xs px-2.5 py-2 rounded-lg transition-colors w-full"
                      style="background: var(--color-surface); border: 1px solid var(--color-border); color: var(--color-on-surface);"
                      onmouseenter={(e) => (e.currentTarget as HTMLElement).style.borderColor = 'var(--color-primary)'}
                      onmouseleave={(e) => (e.currentTarget as HTMLElement).style.borderColor = 'var(--color-border)'}
                    >
                      <div class="font-medium" style="color: var(--color-primary);">{link.other_page_title}</div>
                      <div class="flex items-center justify-between gap-2 mt-0.5">
                        <span class="text-xs px-1.5 py-0.5 rounded flex-shrink-0"
                          style="background: var(--color-surface-lo); color: var(--color-on-muted); border: 1px solid var(--color-border);"
                        >{link.relationship}</span>
                      </div>
                      {#if link.description}
                        <p class="text-xs leading-relaxed mt-1" style="color: var(--color-on-muted);">{link.description}</p>
                      {/if}
                    </button>
                  {/each}
                </div>
              </section>
            {/if}

            <!-- Footer timestamp -->
            <p class="text-xs mt-auto pt-2" style="color: var(--color-on-muted); border-top: 1px solid var(--color-border);">
              Synthesized {synthesis.synthesized_at.slice(0, 16)}
            </p>
          {/if}
        {/if}
      </div>
    </aside>
  {/if}

</div>

<style>
  :global(.milkdown) {
    background: transparent !important;
    box-shadow: none !important;
    padding: 2rem 3rem !important;
    max-width: 1100px !important;
    margin: 0 auto !important;
    color: var(--color-on-surface) !important;
  }
  :global(.milkdown .editor) {
    color: var(--color-on-surface) !important;
  }
  :global(.milkdown h1, .milkdown h2, .milkdown h3, .milkdown h4) {
    color: var(--color-on-surface) !important;
  }
  :global(.milkdown a) {
    color: var(--color-primary) !important;
  }
  :global(.milkdown code) {
    background: var(--color-surface-hi) !important;
    color: var(--color-primary-dim) !important;
  }
  :global(.milkdown pre) {
    background: var(--color-surface-lowest) !important;
  }
  :global(.milkdown p, .milkdown li, .milkdown td, .milkdown th) {
    color: var(--color-on-surface) !important;
  }
  :global(.milkdown blockquote) {
    border-left-color: var(--color-border) !important;
    color: var(--color-on-muted) !important;
  }
</style>
