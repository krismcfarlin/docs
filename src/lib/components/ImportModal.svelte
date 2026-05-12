<script lang="ts">
  import { currentSpace, spaces, pages, currentPage, currentVersion, versions } from '$lib/stores';
  import { fetchGdoc, importPage, getPageVersion, listPageVersions, createSpace, createPage, startGoogleOAuth, waitGoogleOAuthCallback, saveSettings, getSettings } from '$lib/api';
  import { invoke } from '@tauri-apps/api/core';
  import { activityStart, activityDone, activityError } from '$lib/stores';

  let { onclose }: { onclose: () => void } = $props();

  type Tab = 'file' | 'gdoc' | 'url';
  let tab = $state<Tab>('file');

  // ── File tab ───────────────────────────────────────────────────────────
  let title   = $state('');
  let content = $state('');
  let status  = $state('');
  let loading = $state(false);
  let dragOver = $state(false);
  let fileInput: HTMLInputElement;

  function pickFile() { fileInput.click(); }

  function readFile(file: File) {
    if (!title) title = file.name.replace(/\.(md|txt|markdown)$/i, '');
    const reader = new FileReader();
    reader.onload  = () => { content = reader.result as string; status = `Loaded: ${file.name}`; };
    reader.onerror = () => { status = `Read error: ${reader.error}`; };
    reader.readAsText(file);
  }

  function onFileChange(e: Event) {
    const file = (e.target as HTMLInputElement).files?.[0];
    if (file) readFile(file);
  }

  function onDragEnter(e: DragEvent) { e.preventDefault(); e.stopPropagation(); dragOver = true; }
  function onDragOver(e: DragEvent)  { e.preventDefault(); e.stopPropagation(); if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy'; }
  function onDragLeave(e: DragEvent) { e.preventDefault(); e.stopPropagation(); dragOver = false; }
  function onDrop(e: DragEvent) {
    e.preventDefault(); e.stopPropagation(); dragOver = false;
    const files = Array.from(e.dataTransfer?.files ?? []);
    if (files.length > 0) { readFile(files[0]); return; }
    const item = Array.from(e.dataTransfer?.items ?? []).find(i => i.kind === 'file');
    if (item) { const f = item.getAsFile(); if (f) { readFile(f); return; } }
    status = 'Drop failed — try the file picker button.';
  }

  // ── URL tab ────────────────────────────────────────────────────────────
  let urlInput = $state('');
  let urlStatus = $state('');
  let urlLoading = $state(false);

  // ── Google Docs tab ────────────────────────────────────────────────────
  type GDocFile = { id: string; name: string; modified_time: string };

  let gdocList          = $state<GDocFile[]>([]);
  let gdocLoading       = $state(false);
  let gdocStatus        = $state('');
  let selected          = $state<Set<string>>(new Set());
  let listLoaded        = $state(false);
  let needsReauth       = $state(false);
  let reconnectingGoogle = $state(false);

  async function reconnectGoogle() {
    reconnectingGoogle = true;
    needsReauth = false;
    gdocStatus = 'Waiting for browser sign-in…';
    try {
      await startGoogleOAuth('');
      const tokens = await waitGoogleOAuthCallback('', '');
      // Persist tokens without wiping other settings
      const s = await getSettings();
      await saveSettings(
        s.sqld_url ?? null,
        s.sqld_token ?? null,
        s.google_client_id ?? null,
        s.google_client_secret ?? null,
        tokens.access_token,
        tokens.refresh_token,
        s.user_name ?? null,
        s.user_email ?? null,
      );
      gdocStatus = '';
      await loadDocList();
    } catch (e) {
      gdocStatus = `Sign-in failed: ${e}`;
      needsReauth = true;
    } finally {
      reconnectingGoogle = false;
    }
  }

  async function loadDocList() {
    gdocLoading = true;
    gdocStatus  = '';
    needsReauth = false;
    try {
      gdocList   = await invoke<GDocFile[]>('list_gdocs');
      listLoaded = true;
      if (gdocList.length === 0) gdocStatus = 'No Google Docs found in your Drive.';
    } catch (e: any) {
      const msg = String(e);
      if (msg.includes('Not connected')) {
        gdocStatus = 'Connect your Google account to import private docs.';
        needsReauth = true;
      } else if (msg.includes('invalid_grant') || msg.includes('refresh failed') || msg.includes('session revoked')) {
        gdocStatus = 'Google session expired. Click Reconnect to sign in again.';
        needsReauth = true;
      } else {
        gdocStatus = `Error: ${e}`;
      }
    } finally {
      gdocLoading = false;
    }
  }

  function toggleSelect(id: string) {
    console.log('[toggleSelect] called with id=', id, 'current size=', selected.size);
    const s = new Set(selected);
    if (s.has(id)) s.delete(id); else s.add(id);
    selected = s;
    console.log('[toggleSelect] new size=', selected.size, 'has id=', selected.has(id));
  }

  function toggleAll() {
    selected = selected.size === gdocList.length
      ? new Set()
      : new Set(gdocList.map(f => f.id));
  }

  /** Split markdown by H1 headings → [{title, content}] */
  function splitByH1(md: string, fallbackTitle: string): { title: string; content: string }[] {
    const lines = md.split('\n');
    const sections: { title: string; content: string }[] = [];
    let current: { title: string; lines: string[] } | null = null;

    for (const line of lines) {
      const h1 = line.match(/^#\s+(.+)/);
      if (h1) {
        if (current) sections.push({ title: current.title, content: current.lines.join('\n').trim() });
        current = { title: h1[1].trim(), lines: [line] };
      } else {
        if (!current) current = { title: fallbackTitle, lines: [] };
        current.lines.push(line);
      }
    }
    if (current) sections.push({ title: current.title, content: current.lines.join('\n').trim() });
    return sections.filter(s => s.content.length > 0 || s.title !== fallbackTitle);
  }

  type DocTab = { title: string; content: string };
  type GDocImport = { doc_title: string; tabs: DocTab[] };

  async function importSelected() {
    console.log('[importSelected] selected=', selected.size, 'space=', $currentSpace?.id);
    if (selected.size === 0 || !$currentSpace) return;
    loading = true;
    const actId = activityStart(`Importing ${selected.size} Google Doc${selected.size > 1 ? 's' : ''}…`);
    let totalPages = 0;

    try {
      for (const fileId of selected) {
        const file = gdocList.find(f => f.id === fileId)!;
        gdocStatus = `Fetching "${file.name}"…`;

        // Use Docs API to get per-tab content
        const gdoc: GDocImport = await invoke<GDocImport>('fetch_gdoc_tabs', { fileId });
        const { doc_title, tabs } = gdoc;
        const spaceId = $currentSpace!.id;

        let firstPageId: string | null = null;

        if (tabs.length === 1) {
          // Single tab — import directly at root
          const [id]: string[] = await invoke('import_pages_bulk', {
            spaceId,
            pages: [{ title: tabs[0].title || doc_title, content: tabs[0].content, parent_page_id: null }],
          });
          firstPageId = id;
          $pages = [...$pages, { id, title: tabs[0].title || doc_title, space_id: spaceId, creator_id: 'user_demo_001', parent_page_id: null, sort_order: 0, created_at: '', updated_at: '' }];
        } else {
          // Multiple tabs — create folder page, then child pages under it
          gdocStatus = `Creating folder "${doc_title}" with ${tabs.length} tabs…`;
          const folder = await createPage(doc_title, spaceId, undefined);
          $pages = [...$pages, folder];
          const childIds: string[] = await invoke('import_pages_bulk', {
            spaceId,
            pages: tabs.map(t => ({ title: t.title, content: t.content, parent_page_id: folder.id })),
          });
          $pages = [
            ...$pages,
            ...childIds.map((id, i) => ({
              id, title: tabs[i].title, space_id: spaceId,
              creator_id: 'user_demo_001', parent_page_id: folder.id, sort_order: i, created_at: '', updated_at: '',
            })),
          ];
          firstPageId = childIds[0] ?? folder.id;
          totalPages += childIds.length - 1;
        }

        if (firstPageId) {
          const ver = await getPageVersion(firstPageId, spaceId);
          const vlist = await listPageVersions(firstPageId, spaceId);
          $currentPage    = $pages.find(p => p.id === firstPageId)!;
          $currentVersion = ver;
          $versions       = vlist;
        }
        totalPages += tabs.length;
      }

      activityDone(actId, `Imported ${totalPages} page${totalPages !== 1 ? 's' : ''}`);
      onclose();
    } catch (e) {
      console.error('[importSelected] ERROR:', e);
      activityError(actId, String(e));
      gdocStatus = `Error: ${e}`;
    } finally {
      loading = false;
    }
  }

  // ── File import ────────────────────────────────────────────────────────
  async function doImport() {
    if (!content.trim()) { status = 'No content to import'; return; }
    if (!title.trim())   { status = 'Please enter a title'; return; }
    if (!$currentSpace)  { status = 'Select a space first'; return; }
    loading = true;
    status  = 'Importing…';
    try {
      const pageId = await importPage(title.trim(), $currentSpace.id, content.trim());
      const ver    = await getPageVersion(pageId, $currentSpace.id);
      const vlist  = await listPageVersions(pageId, $currentSpace.id);
      $currentPage    = { id: pageId, title: title.trim(), space_id: $currentSpace.id, creator_id: 'user_demo_001', parent_page_id: null, sort_order: 0, created_at: '', updated_at: '' };
      $currentVersion = ver;
      $versions       = vlist;
      $pages = [...$pages, $currentPage];
      onclose();
    } catch (e) {
      status = `Import failed: ${e}`;
    } finally {
      loading = false;
    }
  }

  async function importFromUrl() {
    if (!urlInput.trim()) { urlStatus = 'Paste a Google Doc URL'; return; }
    if (!$currentSpace) { urlStatus = 'Select a space first'; return; }
    urlLoading = true;
    urlStatus = 'Fetching…';
    const actId = activityStart('Importing from URL…');
    try {
      // Extract doc ID from URL to use fetch_gdoc_tabs (all tabs + images via Docs API).
      // Fall back to fetchGdoc (public markdown export) if no Google auth.
      const urlStr = urlInput.trim();
      const idMatch = urlStr.match(/\/d\/([^/?#]+)/);
      const fileId = idMatch?.[1] ?? '';

      let tabs: DocTab[] = [];
      let doc_title = 'Imported Doc';
      try {
        const gdoc: GDocImport = await invoke<GDocImport>('fetch_gdoc_tabs', { fileId });
        tabs = gdoc.tabs;
        doc_title = gdoc.doc_title;
      } catch {
        // No Google auth or Docs API failed — fall back to markdown export
        const md = await fetchGdoc(urlStr);
        const firstH1 = md.match(/^#\s+(.+)/m);
        doc_title = firstH1 ? firstH1[1].trim() : 'Imported Doc';
        tabs = [{ title: doc_title, content: md }];
      }

      const spaceId = $currentSpace.id;
      let firstPageId: string;

      if (tabs.length === 1) {
        const [id]: string[] = await invoke('import_pages_bulk', {
          spaceId,
          pages: [{ title: tabs[0].title || doc_title, content: tabs[0].content, parent_page_id: null }],
        });
        firstPageId = id;
        $pages = [...$pages, { id, title: tabs[0].title || doc_title, space_id: spaceId, creator_id: 'user_demo_001', parent_page_id: null, sort_order: 0, created_at: '', updated_at: '' }];
      } else {
        // Multiple tabs — folder page + children
        urlStatus = `Creating folder "${doc_title}" with ${tabs.length} tabs…`;
        const folder = await createPage(doc_title, spaceId, undefined);
        $pages = [...$pages, folder];
        const childIds: string[] = await invoke('import_pages_bulk', {
          spaceId,
          pages: tabs.map(t => ({ title: t.title, content: t.content, parent_page_id: folder.id })),
        });
        $pages = [
          ...$pages,
          ...childIds.map((id, i) => ({
            id, title: tabs[i].title, space_id: spaceId,
            creator_id: 'user_demo_001', parent_page_id: folder.id, sort_order: i, created_at: '', updated_at: '',
          })),
        ];
        firstPageId = childIds[0] ?? folder.id;
      }

      $currentPage = $pages.find(p => p.id === firstPageId)!;
      $currentVersion = await getPageVersion(firstPageId, spaceId);
      $versions = await listPageVersions(firstPageId, spaceId);

      activityDone(actId, `Imported ${tabs.length} page${tabs.length !== 1 ? 's' : ''} from "${doc_title}"`);
      onclose();
    } catch (e) {
      urlStatus = `Error: ${e}`;
      activityError(actId, String(e));
    } finally {
      urlLoading = false;
    }
  }

  function fmt(iso: string) {
    if (!iso) return '';
    try { return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' }); }
    catch { return ''; }
  }

  function onKeydown(e: KeyboardEvent) { if (e.key === 'Escape') onclose(); }
</script>

<!-- Backdrop -->
{#if !$currentSpace}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
    role="dialog" aria-modal="true" onclick={onclose}>
    <div class="bg-slate-900 border border-slate-700 rounded-xl p-8 text-center shadow-2xl max-w-sm mx-4">
      <p class="text-slate-200 font-semibold mb-1">No space selected</p>
      <p class="text-slate-500 text-sm mb-4">Select or create a space before importing.</p>
      <button onclick={onclose}
        class="px-4 py-2 bg-slate-700 hover:bg-slate-600 text-slate-200 text-sm rounded-lg transition-colors">Close</button>
    </div>
  </div>
{:else}
<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
  role="dialog" aria-modal="true" tabindex="-1"
  onkeydown={onKeydown}
>
  <input bind:this={fileInput} type="file" accept=".md,.txt,.markdown" class="hidden" onchange={onFileChange} />

  <div class="bg-slate-900 border border-slate-700 rounded-xl shadow-2xl w-full max-w-xl mx-4 flex flex-col overflow-hidden max-h-[85vh]">

    <!-- Header -->
    <div class="flex items-center justify-between px-5 py-4 border-b border-slate-700 shrink-0">
      <h2 class="text-slate-200 font-semibold">Import</h2>
      <button onclick={onclose} class="text-slate-500 hover:text-slate-300 text-lg leading-none">✕</button>
    </div>

    <!-- Tabs -->
    <div class="flex border-b border-slate-700 shrink-0">
      <button
        onclick={() => tab = 'file'}
        class="px-5 py-2.5 text-sm font-medium transition-colors {tab === 'file' ? 'text-slate-100 border-b-2 border-accent -mb-px' : 'text-slate-500 hover:text-slate-300'}"
      >Markdown file</button>
      <button
        onclick={() => { tab = 'gdoc'; if (!listLoaded) loadDocList(); }}
        class="px-5 py-2.5 text-sm font-medium transition-colors {tab === 'gdoc' ? 'text-slate-100 border-b-2 border-accent -mb-px' : 'text-slate-500 hover:text-slate-300'}"
      >Google Docs</button>
      <button
        onclick={() => { tab = 'url'; }}
        class="px-5 py-2.5 text-sm font-medium transition-colors {tab === 'url' ? 'text-slate-100 border-b-2 border-accent -mb-px' : 'text-slate-500 hover:text-slate-300'}"
      >From URL</button>
    </div>

    <!-- Body -->
    <div class="flex-1 overflow-y-auto">

      {#if tab === 'file'}
        <div class="p-5 space-y-4">
          <div
            role="button" tabindex="0"
            class="w-full border-2 border-dashed rounded-lg py-8 text-center text-sm transition-colors cursor-pointer select-none
              {dragOver ? 'border-accent bg-accent/10 text-accent' : 'border-slate-700 text-slate-400 hover:border-slate-500 hover:text-slate-200'}"
            ondragenter={onDragEnter} ondragover={onDragOver} ondragleave={onDragLeave} ondrop={onDrop}
            onclick={pickFile}
            onkeyup={(e) => { if (e.key === 'Enter' || e.key === ' ') pickFile(); }}
          >{dragOver ? 'Drop to import' : 'Drop a .md / .txt file here, or click to browse'}</div>

          <input type="text" bind:value={title} placeholder="Page title"
            class="w-full bg-slate-800 text-slate-200 text-sm px-3 py-2 rounded outline-none border border-slate-700 focus:border-accent" />

          {#if content}
            <div class="bg-slate-950 rounded p-3 max-h-28 overflow-y-auto">
              <pre class="text-xs text-slate-400 whitespace-pre-wrap font-mono">{content.slice(0, 600)}{content.length > 600 ? '\n…' : ''}</pre>
            </div>
          {/if}
          {#if status}
            <p class="text-xs {status.startsWith('Error') || status.startsWith('Import failed') ? 'text-red-400' : 'text-slate-400'}">{status}</p>
          {/if}
        </div>

      {:else if tab === 'url'}
        <div class="p-5 space-y-4">
          <p class="text-xs text-slate-400">Paste a Google Doc URL. Works for docs shared "Anyone with the link" — or connect Google in Settings for private docs.</p>
          <input
            type="text"
            bind:value={urlInput}
            placeholder="https://docs.google.com/document/d/…"
            class="w-full bg-slate-800 text-slate-200 text-sm px-3 py-2 rounded outline-none border border-slate-700 focus:border-accent font-mono"
            onkeyup={(e) => { if (e.key === 'Enter') importFromUrl(); }}
          />
          {#if urlStatus}
            <p class="text-xs {urlStatus.startsWith('Error') ? 'text-red-400' : 'text-slate-400'}">{urlStatus}</p>
          {/if}
        </div>

      {:else}
        <!-- Google Docs picker -->
        <div class="flex flex-col h-full">

          {#if gdocLoading}
            <div class="flex items-center justify-center py-12 text-slate-500 text-sm">Loading your docs…</div>

          {:else if gdocStatus && !listLoaded}
            <div class="p-5 flex flex-col gap-3">
              <p class="text-sm text-amber-400">{gdocStatus}</p>
              {#if needsReauth}
                <button
                  onclick={reconnectGoogle}
                  disabled={reconnectingGoogle}
                  class="self-start flex items-center gap-2 px-3 py-1.5 text-xs rounded-lg font-semibold text-white disabled:opacity-50 transition-colors"
                  style="background: #4285f4;"
                >
                  {#if reconnectingGoogle}
                    <span class="animate-spin w-3 h-3 border border-white border-t-transparent rounded-full inline-block"></span>
                    Waiting for browser…
                  {:else}
                    Reconnect Google
                  {/if}
                </button>
              {:else}
                <button onclick={loadDocList} class="self-start text-xs text-slate-400 hover:text-slate-200 underline">Retry</button>
              {/if}
            </div>

          {:else if gdocList.length > 0}
            <!-- Toolbar -->
            <div class="flex items-center justify-between px-4 py-2 border-b border-slate-800 shrink-0">
              <button onclick={toggleAll} class="text-xs text-slate-500 hover:text-slate-300 transition-colors">
                {selected.size === gdocList.length ? 'Deselect all' : 'Select all'}
              </button>
              <span class="text-xs text-slate-600">{selected.size} selected</span>
            </div>

            <!-- Doc list -->
            <div class="flex-1 overflow-y-auto divide-y divide-slate-800">
              {#each gdocList as doc}
                <button
                  onclick={(e) => { e.stopPropagation(); toggleSelect(doc.id); }}
                  class="w-full flex items-center gap-3 px-4 py-3 text-left hover:bg-slate-800/50 transition-colors"
                >
                  <div class="w-4 h-4 rounded border shrink-0 flex items-center justify-center
                    {selected.has(doc.id) ? 'bg-accent border-accent' : 'border-slate-600'}">
                    {#if selected.has(doc.id)}
                      <svg class="w-2.5 h-2.5 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="3">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
                      </svg>
                    {/if}
                  </div>
                  <div class="flex-1 min-w-0">
                    <p class="text-sm text-slate-200 truncate">{doc.name}</p>
                    <p class="text-xs text-slate-600 mt-0.5">Modified {fmt(doc.modified_time)}</p>
                  </div>
                </button>
              {/each}
            </div>

            {#if gdocStatus}
              <p class="px-4 py-2 text-xs text-slate-400 shrink-0">{gdocStatus}</p>
            {/if}

          {:else if listLoaded}
            <div class="flex flex-col items-center justify-center py-12 gap-2">
              <p class="text-slate-500 text-sm">No Google Docs found.</p>
              <button onclick={loadDocList} class="text-xs text-slate-500 hover:text-slate-300 underline">Refresh</button>
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <!-- Footer -->
    <div class="flex justify-between items-center gap-2 px-5 py-4 border-t border-slate-700 shrink-0">
      <div>
        {#if tab === 'gdoc' && selected.size > 0}
          <p class="text-xs text-slate-500">Docs with multiple H1 headings will be split into separate pages.</p>
        {/if}
      </div>
      <div class="flex flex-col items-end gap-1">
        {#if tab === 'gdoc'}
          <div class="flex gap-2 text-xs font-mono">
            <span class={selected.size === 0 ? 'text-red-400' : 'text-green-500'}>
              selected={selected.size}
            </span>
            <span class={!$currentSpace ? 'text-red-400' : 'text-green-500'}>
              space={$currentSpace ? $currentSpace.name : 'NONE'}
            </span>
            <span class={loading ? 'text-yellow-400' : 'text-green-500'}>
              loading={String(loading)}
            </span>
          </div>
        {/if}
        <div class="flex gap-2">
          <button onclick={onclose} class="px-4 py-2 text-sm text-slate-400 hover:text-slate-200 transition-colors">Cancel</button>
          {#if tab === 'file'}
            <button onclick={doImport} disabled={loading || !content}
              class="px-4 py-2 text-sm bg-accent hover:bg-accent-hover text-white rounded transition-colors disabled:opacity-40">
              Import
            </button>
          {:else if tab === 'url'}
            <button onclick={importFromUrl} disabled={urlLoading || !urlInput.trim()}
              class="px-4 py-2 text-sm bg-accent hover:bg-accent-hover text-white rounded transition-colors disabled:opacity-40">
              {urlLoading ? 'Importing…' : 'Import'}
            </button>
          {:else}
            <button onclick={importSelected} disabled={loading || selected.size === 0 || !$currentSpace}
              class="px-4 py-2 text-sm bg-accent hover:bg-accent-hover text-white rounded transition-colors disabled:opacity-40">
              {loading ? 'Importing…' : `Import ${selected.size > 0 ? selected.size : ''} doc${selected.size !== 1 ? 's' : ''}`}
            </button>
          {/if}
        </div>
      </div>
    </div>
  </div>
</div>
{/if}
