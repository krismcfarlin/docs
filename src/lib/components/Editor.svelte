<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Crepe } from '@milkdown/crepe';
  import { replaceAll } from '@milkdown/kit/utils';
  import { listenerCtx } from '@milkdown/kit/plugin/listener';
  import '@milkdown/crepe/theme/common/style.css';
  import '@milkdown/crepe/theme/frame-dark.css';
  import mermaid from 'mermaid';
  import { currentVersion, currentPage, currentSpace, readMode, theme, userName, lastSynthesisAt, activityStart, activityDone, activityError } from '$lib/stores';
  import { savePageVersion, type SaveResult, upsertPresence, clearPresence, getPagePresence, type Presence, getPageSynthesis, getPageLinks, synthesizePage, type PageSynthesis, type PageLink, getPageImage } from '$lib/api';

  async function inlineImages(md: string): Promise<string> {
    const refs = [...md.matchAll(/spaceimg:\/\/([^\s)]+)/g)];
    if (refs.length === 0) return md;
    const replacements = await Promise.all(
      refs.map(async ([full, id]) => {
        try { return [full, await getPageImage(id)] as [string, string]; }
        catch { return [full, full] as [string, string]; }
      })
    );
    let out = md;
    for (const [orig, dataUri] of replacements) out = out.replaceAll(orig, dataUri);
    return out;
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
    if (!page || space?.source !== 'remote') { synthesis = null; pageLinks = []; return; }
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
  <div class="flex items-center justify-between px-4 py-1 border-b border-slate-800 flex-shrink-0 {$readMode ? 'bg-amber-900/10' : ''}">
    {#if !$readMode}
      <div class="flex rounded overflow-hidden border border-slate-700 text-xs">
        <button
          onclick={() => mode !== 'wysiwyg' && toggleMode()}
          class="px-3 py-1 transition-colors {mode === 'wysiwyg' ? 'bg-indigo-600 text-white' : 'text-slate-400 hover:text-slate-200'}"
        >Rich</button>
        <button
          onclick={() => mode !== 'source' && toggleMode()}
          class="px-3 py-1 transition-colors {mode === 'source' ? 'bg-indigo-600 text-white' : 'text-slate-400 hover:text-slate-200'}"
        >Markdown</button>
      </div>
    {:else}
      <span class="text-xs text-amber-500">Read only — click <strong class="text-amber-400">Edit</strong> in the toolbar to make changes</span>
    {/if}
    {#if $currentSpace?.source === 'remote'}
      <button
        onclick={() => showInsights = !showInsights}
        class="ml-2 px-2 py-1 rounded text-xs transition-colors {showInsights ? 'bg-indigo-600 text-white' : 'text-slate-400 hover:text-slate-200 border border-slate-700'}"
      >Insights</button>
    {/if}
  </div>

  {#if pagePresence.filter(p => p.user_name !== $userName).length > 0}
    <div class="flex items-center gap-2 px-4 py-1 border-b border-slate-800 bg-slate-900/50 flex-shrink-0">
      <span class="text-xs text-slate-500">Also here:</span>
      {#each pagePresence.filter(p => p.user_name !== $userName) as p}
        <span class="flex items-center gap-1 text-xs px-2 py-0.5 rounded-full
          {p.status === 'editing' ? 'bg-green-900/50 text-green-300' : 'bg-slate-800 text-slate-400'}">
          {#if p.status === 'editing'}
            <span class="w-1.5 h-1.5 rounded-full bg-green-400 inline-block"></span>
          {:else}
            <span class="w-1.5 h-1.5 rounded-full bg-slate-500 inline-block"></span>
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
    class:pointer-events-none={$readMode}
    style:display={mode === 'wysiwyg' ? 'block' : 'none'}
  ></div>

  <!-- Source: editable markdown textarea -->
  {#if mode === 'source'}
    <textarea
      class="flex-1 bg-transparent text-slate-200 font-mono text-sm p-8 resize-none
             outline-none border-none leading-relaxed"
      bind:value={markdownSource}
      oninput={() => scheduleAutoSave(markdownSource)}
      readonly={$readMode}
      placeholder="Write markdown here..."
      spellcheck={false}
    ></textarea>
  {/if}

  {#if conflict}
    <div class="fixed inset-0 z-50 flex flex-col bg-[#1e1e2e]/95 p-6 gap-4">
      <div class="text-amber-400 font-semibold text-sm">
        ⚠ Conflict — this document was edited by someone else while you were writing
      </div>
      <div class="flex flex-1 gap-4 min-h-0 overflow-hidden">
        <div class="flex-1 flex flex-col gap-2 min-h-0">
          <div class="text-xs text-slate-400 font-semibold uppercase tracking-wide">Your version</div>
          <textarea readonly class="flex-1 font-mono text-sm bg-slate-900 text-slate-200 p-4 rounded resize-none overflow-y-auto" value={myContent}></textarea>
        </div>
        <div class="flex-1 flex flex-col gap-2 min-h-0">
          <div class="text-xs text-slate-400 font-semibold uppercase tracking-wide">Their version (server)</div>
          <textarea readonly class="flex-1 font-mono text-sm bg-slate-900 text-slate-200 p-4 rounded resize-none overflow-y-auto" value={conflict.current_content}></textarea>
        </div>
      </div>
      <div class="flex gap-3 justify-end">
        <button onclick={keepTheirs} class="px-4 py-2 rounded bg-slate-700 hover:bg-slate-600 text-sm text-slate-200">Keep Theirs</button>
        <button onclick={keepMine} class="px-4 py-2 rounded bg-indigo-600 hover:bg-indigo-500 text-sm text-white">Keep Mine (overwrite)</button>
      </div>
    </div>
  {/if}

  </div><!-- end main editor column -->

  <!-- Insights panel -->
  {#if showInsights && $currentSpace?.source === 'remote'}
    <aside class="w-72 border-l border-slate-800 bg-slate-900/60 flex flex-col overflow-y-auto flex-shrink-0 text-sm p-4 gap-4">
      {#if !synthesis}
        <div class="flex flex-col gap-3">
          <p class="text-slate-400 text-xs font-semibold uppercase tracking-wide">Insights</p>
          <p class="text-slate-500 text-xs leading-relaxed">No synthesis data for this page yet.</p>
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
            class="px-3 py-1.5 rounded text-xs bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-50 flex items-center gap-1.5"
          >
            {#if synthesizing}
              <span class="w-3 h-3 border border-white border-t-transparent rounded-full animate-spin inline-block"></span>
              Synthesizing…
            {:else}
              ✦ Synthesize this page
            {/if}
          </button>
        </div>
      {:else}
        <section>
          <h3 class="text-xs font-semibold text-slate-400 uppercase tracking-wide mb-1.5">Summary</h3>
          <p class="text-slate-300 text-xs leading-relaxed">{synthesis.summary}</p>
        </section>

        {#if synthesis.key_points.length > 0}
          <section>
            <h3 class="text-xs font-semibold text-slate-400 uppercase tracking-wide mb-1.5">Key Points</h3>
            <ul class="space-y-1.5">
              {#each synthesis.key_points as point}
                <li class="text-xs text-slate-300 flex gap-1.5 leading-relaxed">
                  <span class="text-indigo-400 flex-shrink-0 mt-0.5">•</span>{point}
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        {#if synthesis.topics.length > 0}
          <section>
            <h3 class="text-xs font-semibold text-slate-400 uppercase tracking-wide mb-1.5">Topics</h3>
            <div class="flex flex-wrap gap-1">
              {#each synthesis.topics as topic}
                <span class="px-2 py-0.5 rounded-full bg-slate-800 text-slate-400 text-xs">{topic}</span>
              {/each}
            </div>
          </section>
        {/if}

        {#if pageLinks.length > 0}
          <section>
            <h3 class="text-xs font-semibold text-slate-400 uppercase tracking-wide mb-1.5">Related</h3>
            <ul class="space-y-2">
              {#each pageLinks as link}
                <li class="text-xs">
                  <span class="text-indigo-400">{link.other_page_title}</span>
                  <span class="text-slate-500 ml-1">· {link.relationship}</span>
                  {#if link.description}
                    <p class="text-slate-500 mt-0.5 leading-relaxed">{link.description}</p>
                  {/if}
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        <p class="text-slate-600 text-xs mt-auto pt-2 border-t border-slate-800">Synthesized {synthesis.synthesized_at.slice(0, 16)}</p>
      {/if}
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
    color: #e2e8f0 !important;
  }
  :global(.milkdown .editor) {
    color: #e2e8f0 !important;
  }
  :global(.milkdown h1, .milkdown h2, .milkdown h3) {
    color: #f1f5f9 !important;
  }
  :global(.milkdown a) {
    color: #818cf8 !important;
  }
  :global(.milkdown code) {
    background: #2d2d3f !important;
    color: #a5b4fc !important;
  }
  :global(.milkdown pre) {
    background: #1e1e2e !important;
  }
</style>
