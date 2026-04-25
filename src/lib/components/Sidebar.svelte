<script lang="ts">
  import {
    spaces, currentSpace, pages, currentPage, currentVersion, versions,
    theme, searchFocusTick, userName, userEmail,
    activityStart, activityDone, activityError,
  } from '$lib/stores';
  import {
    getSpaces, createSpace, getPages, createPage,
    getPageVersion, listPageVersions, syncDb, syncSpace,
    searchSimilarPages, type SearchResult,
    deletePage, deleteSpace, renameSpace, type Page, type Space,
    moveSpace, reorderSpaces, reorderPages,
    getTrashPages, restorePage, permanentDeletePage,
    recordPageAccess, connectRemoteSpace,
    getPageSynthesis, synthesizePage, freezeVersion, vectorizePage, updateSpaceOverview,
    movePageToSpace,
  } from '$lib/api';
  import ImportModal from './ImportModal.svelte';
  import ActivityPanel from './ActivityPanel.svelte';
  import SettingsModal from './SettingsModal.svelte';
  import {
    FolderOpen, Folder, FileText, Search, Trash2, Settings,
    Plus, ChevronRight, ChevronDown, Sun, Moon, RefreshCw,
    Pencil, X, User, UploadCloud, Cloud, Lock, Sparkles, MoveRight,
  } from 'lucide-svelte';

  function focusOnMount(el: HTMLElement) { setTimeout(() => el.focus(), 0); }

  let newSpaceName    = $state('');
  let showNewSpace    = $state(false);
  let syncing         = $state(false);
  let showImport      = $state(false);
  let showSettings    = $state(false);
  let showTrash       = $state(false);
  let trashPages      = $state<Page[]>([]);
  let moveModalPage   = $state<Page | null>(null);
  let moveTargetId    = $state('');

  let searchQuery   = $state('');
  let searchResults = $state<SearchResult[]>([]);
  let searching     = $state(false);
  let searchTimer: ReturnType<typeof setTimeout>;
  let searchInput: HTMLInputElement;

  // Auto-refresh remote spaces every 30 seconds — new_remote is always live so
  // re-querying pages is all that's needed; no explicit sync call required.
  const SYNC_INTERVAL_MS = 30_000;
  $effect(() => {
    const space = $currentSpace;
    if (space?.source !== 'remote') return;
    console.log('[sidebar] starting auto-refresh interval for remote space', space.id);
    const timer = setInterval(async () => {
      try {
        if ($currentSpace?.id === space.id) {
          console.log('[sidebar] auto-refresh: fetching pages for space', space.id);
          const updated = await getPages(space.id);
          console.log('[sidebar] auto-refresh: got', updated.length, 'pages for space', space.id);
          $pages = updated;
        }
      } catch (e) {
        console.warn('[sidebar] auto-refresh error for space', space.id, e);
      }
    }, SYNC_INTERVAL_MS);
    return () => {
      console.log('[sidebar] clearing auto-refresh interval for space', space.id);
      clearInterval(timer);
    };
  });

  let expandedIds      = $state<Set<string>>(new Set());
  let expandedSpaceIds = $state<Set<string>>(new Set());
  let newChildState    = $state<Record<string, { title: string; show: boolean }>>({});

  let renamingSpaceId   = $state<string | null>(null);
  let renamingSpaceName = $state('');

  // Per-space inline forms
  let newPageForSpace    = $state<Record<string, boolean>>({});
  let newSubSpaceFor     = $state<Record<string, { name: string; error: string }>>({});
  let connectChildFor    = $state<Record<string, { url: string; name: string; token: string; permission: string; connecting: boolean }>>({});

  let dragId     = $state<string | null>(null);
  let dragType   = $state<'space' | 'page' | null>(null);
  let dropTarget = $state<{ id: string; position: 'above' | 'below' | 'inside' } | null>(null);

  // Cmd+K focus
  $effect(() => {
    const tick = $searchFocusTick;
    if (tick && searchInput) { searchInput.focus(); searchInput.select(); }
  });

  // ── Tree builders ─────────────────────────────────────────────────────────
  type SpaceNode = { space: Space; depth: number; hasChildren: boolean };
  type PageNode  = { page: Page;  depth: number; hasChildren: boolean };

  function buildSpaceFlat(all: Space[], parentId: string | null, depth: number): SpaceNode[] {
    const validIds = new Set(all.map(s => s.id));
    const kids = all.filter(s => {
      // Remote spaces and spaces whose parent no longer exists → treat as root.
      const raw = s.source === 'remote' ? null : (s.parent_space_id ?? null);
      const effectiveParent = (raw && !validIds.has(raw)) ? null : raw;
      return effectiveParent === parentId;
    }).sort((a, b) => a.sort_order - b.sort_order);
    return kids.flatMap(space => {
      const node: SpaceNode = { space, depth, hasChildren: all.some(s => (s.parent_space_id ?? null) === space.id) };
      return expandedSpaceIds.has(space.id) ? [node, ...buildSpaceFlat(all, space.id, depth + 1)] : [node];
    });
  }

  function buildFlat(all: Page[], parentId: string | null, depth: number): PageNode[] {
    const kids = all.filter(p => (p.parent_page_id ?? null) === parentId)
                    .sort((a, b) => a.sort_order - b.sort_order);
    return kids.flatMap(page => {
      const node: PageNode = { page, depth, hasChildren: all.some(p => (p.parent_page_id ?? null) === page.id) };
      return expandedIds.has(page.id) ? [node, ...buildFlat(all, page.id, depth + 1)] : [node];
    });
  }

  function spaceNodes(): SpaceNode[] { return buildSpaceFlat($spaces, null, 0); }
  function treeNodes():  PageNode[]  { return buildFlat($pages, null, 0); }
  let _pageNodes = $derived(buildFlat($pages, null, 0));

  function toggle(set: Set<string>, id: string): Set<string> {
    const next = new Set(set); next.has(id) ? next.delete(id) : next.add(id); return next;
  }
  function showNewChild(pid: string) {
    newChildState = { ...newChildState, [pid]: { title: '', show: true } };
    expandedIds = new Set([...expandedIds, pid]);
  }
  function hideNewChild(pid: string) { const s = { ...newChildState }; delete s[pid]; newChildState = s; }

  // ── Drag ──────────────────────────────────────────────────────────────────
  function dropPos(e: DragEvent): 'above' | 'below' | 'inside' {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const p = (e.clientY - r.top) / r.height;
    return p < 0.3 ? 'above' : p > 0.7 ? 'below' : 'inside';
  }
  function dropCls(id: string): string {
    if (!dropTarget || dropTarget.id !== id) return '';
    return dropTarget.position === 'above' ? 'border-t-2 border-primary'
         : dropTarget.position === 'below' ? 'border-b-2 border-primary'
         : 'ring-2 ring-primary ring-inset';
  }
  function onDragLeave(e: DragEvent) {
    const rel = e.relatedTarget as HTMLElement | null;
    if (!rel || !(e.currentTarget as HTMLElement).contains(rel)) dropTarget = null;
  }
  function startDrag(e: DragEvent, id: string, type: 'space' | 'page') {
    dragId = id; dragType = type; e.dataTransfer!.effectAllowed = 'move';
  }
  function clearDrag() { dragId = null; dragType = null; dropTarget = null; }

  function onSpaceDragOver(e: DragEvent, id: string) {
    if (dragType !== 'space') return; e.preventDefault();
    dropTarget = { id, position: dropPos(e) };
  }
  async function onSpaceDrop(e: DragEvent, targetId: string) {
    e.preventDefault();
    if (!dragId || dragId === targetId || dragType !== 'space') { clearDrag(); return; }
    const src = $spaces.find(s => s.id === dragId)!;
    const tgt = $spaces.find(s => s.id === targetId)!;
    if (!src || !tgt) { clearDrag(); return; }
    const pos = dropTarget?.position ?? 'below';
    if (pos === 'inside') {
      await moveSpace(dragId, targetId);
      expandedSpaceIds = new Set([...expandedSpaceIds, targetId]);
    } else {
      const parentId = tgt.parent_space_id ?? null;
      if ((src.parent_space_id ?? null) !== parentId) await moveSpace(dragId, parentId);
      const siblings = $spaces.filter(s => (s.parent_space_id ?? null) === parentId && s.id !== dragId).sort((a, b) => a.sort_order - b.sort_order);
      const idx = siblings.findIndex(s => s.id === targetId);
      siblings.splice(pos === 'above' ? idx : idx + 1, 0, src);
      await reorderSpaces(siblings.map(s => s.id));
    }
    $spaces = await getSpaces(); clearDrag();
  }

  function onPageDragOver(e: DragEvent, id: string) {
    if (dragType !== 'page') return; e.preventDefault();
    const p = dropPos(e); dropTarget = { id, position: p === 'inside' ? 'below' : p };
  }
  async function onPageDrop(e: DragEvent, targetId: string) {
    e.preventDefault();
    if (!dragId || dragId === targetId || dragType !== 'page') { clearDrag(); return; }
    const all = [...$pages].sort((a, b) => a.sort_order - b.sort_order);
    const [moved] = all.splice(all.findIndex(p => p.id === dragId), 1);
    const tgtIdx = all.findIndex(p => p.id === targetId);
    all.splice(dropTarget?.position === 'above' ? tgtIdx : tgtIdx + 1, 0, moved);
    $pages = all; await reorderPages(all.map(p => p.id), $currentSpace!.id); clearDrag();
  }

  // ── App handlers ──────────────────────────────────────────────────────────
  async function selectSpace(space: Space) {
    $currentSpace = space; $currentPage = null; $currentVersion = null;
    expandedIds = new Set(); newChildState = {};
    showTrash = false; trashPages = [];
    $pages = [];
    const actId = activityStart(`Loading pages: ${space.name}…`);
    try {
      $pages = await getPages(space.id);
      activityDone(actId, `Loaded ${$pages.length} pages in ${space.name}`);
    } catch (e) {
      activityError(actId, `Failed to load pages for ${space.name}: ${e}`);
      $pages = [];
    }
  }

  async function selectPage(page: Page) {
    $currentPage = page;
    // Use the local registry space id (not page.space_id which may be from another
    // instance or the seed) so get_or_open_space_db can find the connection.
    const spaceId = $currentSpace?.id ?? page.space_id;
    try {
      $currentVersion = await getPageVersion(page.id, spaceId);
      $versions = await listPageVersions(page.id, spaceId);
      recordPageAccess(page.id, spaceId).catch(() => {});
    } catch (e) {
      console.error('[selectPage] failed to load version for page', page.id, e);
    }
  }

  async function handleCreateSpace() {
    if (!newSpaceName.trim()) return;
    try {
      const space = await createSpace(newSpaceName.trim());
      $spaces = [...$spaces, space]; newSpaceName = ''; showNewSpace = false;
      await selectSpace(space);
    } catch (e) { console.error(e); }
  }

  function showNewPageFor(spaceId: string) {
    newPageForSpace = { ...newPageForSpace, [spaceId]: true };
  }
  function hideNewPageFor(spaceId: string) {
    newPageForSpace = { ...newPageForSpace, [spaceId]: false };
  }
  function showNewSubSpaceFor(spaceId: string) {
    newSubSpaceFor = { ...newSubSpaceFor, [spaceId]: { name: '', error: '' } };
  }
  function hideNewSubSpaceFor(spaceId: string) {
    const s = { ...newSubSpaceFor };
    delete s[spaceId];
    newSubSpaceFor = s;
  }
  function showConnectChildFor(spaceId: string) {
    connectChildFor = { ...connectChildFor, [spaceId]: { url: '', name: '', token: '', permission: 'read', connecting: false } };
  }
  function hideConnectChildFor(spaceId: string) {
    const s = { ...connectChildFor };
    delete s[spaceId];
    connectChildFor = s;
  }

  async function handleCreatePageForSpace(spaceId: string, title: string) {
    if (!title.trim()) return;
    try {
      const page = await createPage(title.trim(), spaceId, undefined);
      if ($currentSpace?.id === spaceId) {
        $pages = [...$pages, page];
      }
      hideNewPageFor(spaceId);
    } catch (e) {
      console.error(e);
    }
  }

  async function handleCreateSubSpaceFor(spaceId: string) {
    const entry = newSubSpaceFor[spaceId];
    if (!entry?.name.trim()) return;
    const trimmed = entry.name.trim();

    // Unique name check within same parent
    const duplicate = $spaces.some(s => s.parent_space_id === spaceId && s.name.toLowerCase() === trimmed.toLowerCase());
    if (duplicate) {
      newSubSpaceFor = { ...newSubSpaceFor, [spaceId]: { ...entry, error: `"${trimmed}" already exists here` } };
      return;
    }

    try {
      await createSpace(trimmed, undefined, spaceId);
      $spaces = await getSpaces();
      expandedSpaceIds = new Set([...expandedSpaceIds, spaceId]);
      hideNewSubSpaceFor(spaceId);
    } catch (e) {
      newSubSpaceFor = { ...newSubSpaceFor, [spaceId]: { ...entry, error: String(e) } };
    }
  }

  async function handleConnectChildSpace(spaceId: string) {
    const entry = connectChildFor[spaceId];
    if (!entry?.url.trim()) return;
    const nameToUse = entry.name.trim() || new URL(entry.url.trim()).host;
    connectChildFor = { ...connectChildFor, [spaceId]: { ...entry, connecting: true } };
    try {
      const space = await connectRemoteSpace(entry.url.trim(), nameToUse, entry.token.trim(), entry.permission, spaceId);
      $spaces = [...$spaces, space];
      expandedSpaceIds = new Set([...expandedSpaceIds, spaceId]);
      hideConnectChildFor(spaceId);
    } catch (e) {
      console.error(e);
    }
  }

  async function handleCreateTopPage() {
    if (!$currentSpace) return;
    const title = (newChildState['__root__']?.title ?? '').trim();
    if (!title) return;
    try {
      const page = await createPage(title, $currentSpace.id, undefined);
      $pages = [...$pages, page]; hideNewChild('__root__'); await selectPage(page);
    } catch (e) { console.error(e); }
  }

  async function handleCreateChildPage(parentId: string) {
    if (!$currentSpace) return;
    const title = (newChildState[parentId]?.title ?? '').trim();
    if (!title) return;
    try {
      const page = await createPage(title, $currentSpace.id, parentId);
      $pages = [...$pages, page]; hideNewChild(parentId);
      expandedIds = new Set([...expandedIds, parentId]);
      await selectPage(page);
    } catch (e) { console.error(e); }
  }

  async function handleSync() {
    syncing = true;
    const id = activityStart('Sync to server');
    try { await syncDb(); activityDone(id); }
    catch (e) { activityError(id, String(e)); }
    finally { syncing = false; }
  }

  async function handleDeletePage(page: Page) {
    if (!confirm(`Move "${page.title}" to trash?`)) return;
    const id = activityStart(`Delete "${page.title}"`);
    try {
      await deletePage(page.id, $currentSpace?.id ?? page.space_id);
      const toRemove = new Set<string>();
      const collect = (pid: string) => { toRemove.add(pid); $pages.filter(p => p.parent_page_id === pid).forEach(p => collect(p.id)); };
      collect(page.id);
      $pages = $pages.filter(p => !toRemove.has(p.id));
      if ($currentPage && toRemove.has($currentPage.id)) { $currentPage = null; $currentVersion = null; }
      activityDone(id, 'Moved to trash');
    } catch (e) { activityError(id, String(e)); }
  }

  function startRenameSpace(space: Space) { renamingSpaceId = space.id; renamingSpaceName = space.name; }
  async function commitRenameSpace(space: Space) {
    const name = renamingSpaceName.trim(); renamingSpaceId = null;
    if (!name || name === space.name) return;
    await renameSpace(space.id, name);
    $spaces = $spaces.map(s => s.id === space.id ? { ...s, name } : s);
    if ($currentSpace?.id === space.id) $currentSpace = { ...$currentSpace, name };
  }

  async function handleDeleteSpace(space: Space) {
    if (!confirm(`Delete space "${space.name}" and all its pages? This cannot be undone.`)) return;
    const id = activityStart(`Delete space "${space.name}"`);
    try {
      await deleteSpace(space.id);
      $spaces = $spaces.filter(s => s.id !== space.id);
      if ($currentSpace?.id === space.id) { $currentSpace = null; $currentPage = null; $currentVersion = null; $pages = []; }
      activityDone(id);
    } catch (e) { activityError(id, String(e)); }
  }

  async function handleProcessSpace(spaceId: string) {
    const actId = activityStart('Process: loading pages…');
    let spacePages: Page[];
    try {
      spacePages = await getPages(spaceId);
    } catch (e) {
      activityError(actId, `Load failed: ${e}`);
      return;
    }
    if (spacePages.length === 0) {
      activityDone(actId, 'No pages in this space');
      return;
    }
    activityDone(actId, `Processing ${spacePages.length} page${spacePages.length !== 1 ? 's' : ''}…`);
    const actId2 = activityStart(`Processing ${spacePages.length} page${spacePages.length !== 1 ? 's' : ''}…`);
    let analyzed = 0, frozen = 0, skipped = 0, failed = 0;
    for (const page of spacePages) {
      const pageLabel = page.title ?? 'Untitled';
      const pageId = activityStart(`Synthesizing: ${pageLabel}…`);
      try {
        // Analyze (skip if synthesis is current)
        const synth = await getPageSynthesis(spaceId, page.id).catch(() => null);
        if (!synth || !page.updated_at || synth.synthesized_at < page.updated_at) {
          await synthesizePage(spaceId, page.id);
          analyzed++;
          activityDone(pageId, `✓ Synthesized: ${pageLabel}`);
        } else {
          activityDone(pageId, `↷ Skipped (up-to-date): ${pageLabel}`);
        }
        // Freeze + index (skip if already frozen)
        const ver = await getPageVersion(page.id, spaceId);
        if (ver && !ver.is_frozen) {
          await freezeVersion(ver.id, spaceId);
          await vectorizePage(ver.id, spaceId);
          frozen++;
        } else {
          skipped++;
        }
      } catch (e) {
        failed++;
        activityError(pageId, `✕ Failed: ${pageLabel} — ${e}`);
      }
    }
    const summary = `Done: ${analyzed} synthesized, ${frozen} indexed, ${skipped} up-to-date, ${failed} failed`;
    if (failed > 0 && analyzed === 0 && frozen === 0) {
      activityError(actId2, summary);
    } else {
      activityDone(actId2, summary);
    }
    // Always try to update overview if anything was analyzed
    if (analyzed > 0) {
      const ovId = activityStart('Updating knowledge overview…');
      try {
        await updateSpaceOverview(spaceId);
        activityDone(ovId, 'Knowledge overview updated');
      } catch (e) {
        activityError(ovId, `Overview update failed: ${e}`);
      }
    }
  }

  async function toggleTrash() {
    showTrash = !showTrash;
    if (showTrash && $currentSpace) trashPages = await getTrashPages($currentSpace.id);
    else trashPages = [];
  }

  async function handleRestore(page: Page) {
    await restorePage(page.id, $currentSpace?.id ?? page.space_id);
    trashPages = trashPages.filter(p => p.id !== page.id);
    if ($currentSpace) $pages = await getPages($currentSpace.id);
  }

  async function handlePermanentDelete(page: Page) {
    if (!confirm(`Permanently delete "${page.title}"? This cannot be undone.`)) return;
    await permanentDeletePage(page.id, $currentSpace?.id ?? page.space_id);
    trashPages = trashPages.filter(p => p.id !== page.id);
  }

  async function confirmMoveToSpace() {
    if (!moveModalPage || !moveTargetId || !$currentSpace) return;
    const page = moveModalPage;
    const fromId = $currentSpace.id;
    const toId = moveTargetId;
    moveModalPage = null; moveTargetId = '';
    const actId = activityStart(`Moving "${page.title}"…`);
    try {
      await movePageToSpace(page.id, fromId, toId);
      // Remove from current pages list
      const toRemove = new Set<string>();
      const collect = (pid: string) => { toRemove.add(pid); $pages.filter(p => p.parent_page_id === pid).forEach(p => collect(p.id)); };
      collect(page.id);
      $pages = $pages.filter(p => !toRemove.has(p.id));
      if ($currentPage && toRemove.has($currentPage.id)) { $currentPage = null; $currentVersion = null; }
      activityDone(actId, `Moved "${page.title}" to ${$spaces.find(s => s.id === toId)?.name ?? toId}`);
    } catch (e) { activityError(actId, String(e)); }
  }

  function scheduleSearch(q: string) {
    clearTimeout(searchTimer);
    if (!q.trim() || !$currentSpace) { searchResults = []; return; }
    searchTimer = setTimeout(async () => {
      searching = true;
      try { searchResults = await searchSimilarPages($currentSpace!.id, q.trim()); }
      catch { searchResults = []; }
      finally { searching = false; }
    }, 400);
  }
  async function openSearchResult(r: SearchResult) {
    searchQuery = ''; searchResults = [];
    const page = $pages.find(p => p.id === r.page_id);
    if (page) await selectPage(page);
  }
  $effect(() => { scheduleSearch(searchQuery); });

  // Avatar initial
  function initial(name: string) { return (name || 'Y')[0].toUpperCase(); }
</script>

<!-- ═══════════════════════ SIDEBAR ═══════════════════════ -->
<aside class="flex flex-col h-full border-r" style="width:264px; min-width:264px; background:var(--color-sidebar); border-color:var(--color-border);">

  <!-- Brand -->
  <div class="px-5 pt-5 pb-3 flex-shrink-0">
    <button
      onclick={() => { $currentPage = null; }}
      class="font-extrabold text-lg tracking-tight text-on-surface hover:opacity-75 transition-opacity text-left"
    >BAMAKO</button>
    <div class="text-[10px] font-bold uppercase tracking-[0.15em] mt-0.5" style="color:var(--color-on-muted);">Knowledge Workspace</div>
  </div>

  <!-- Search -->
  <div class="px-4 pb-3 flex-shrink-0 relative">
    <div class="flex items-center gap-2 px-3 py-2 rounded-xl" style="background:var(--color-surface-lo); border:1px solid var(--color-border);">
      <Search size={13} style="color:var(--color-on-muted); flex-shrink:0;" />
      <input
        type="text"
        bind:value={searchQuery}
        bind:this={searchInput}
        placeholder="Search… (⌘K)"
        class="flex-1 bg-transparent text-sm outline-none min-w-0"
        style="color:var(--color-on-surface);"
        onkeydown={(e) => { if (e.key === 'Escape') { searchQuery = ''; searchResults = []; } }}
      />
      {#if searching}<span class="text-xs" style="color:var(--color-on-muted);">…</span>{/if}
    </div>

    {#if searchResults.length > 0}
      <div class="absolute left-4 right-4 top-full mt-1 z-50 rounded-xl overflow-hidden shadow-card" style="background:var(--color-surface); border:1px solid var(--color-border);">
        {#each searchResults as r}
          <button onclick={() => openSearchResult(r)}
            class="w-full text-left px-3 py-2.5 transition-colors border-b last:border-0"
            style="border-color:var(--color-border);"
            onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
            onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
          >
            <div class="flex items-center justify-between gap-2 mb-0.5">
              <span class="text-xs font-semibold text-on-surface truncate">{r.title}</span>
              <span class="text-xs font-mono shrink-0" style="color:var(--color-on-muted);">{(r.score * 100).toFixed(0)}%</span>
            </div>
            {#if r.snippet}<p class="text-xs line-clamp-2" style="color:var(--color-on-muted);">{r.snippet}</p>{/if}
          </button>
        {/each}
      </div>
    {:else if searchQuery && !searching}
      <div class="absolute left-4 right-4 top-full mt-1 z-50 rounded-xl px-3 py-2.5 shadow-card" style="background:var(--color-surface); border:1px solid var(--color-border);">
        <span class="text-xs" style="color:var(--color-on-muted);">No frozen pages match. Freeze a page to index it.</span>
      </div>
    {/if}
  </div>

  <!-- Scrollable space + page tree -->
  <div class="flex-1 overflow-y-auto px-3 pb-2">

    {#each spaceNodes() as sn (sn.space.id)}
      {@const si = sn.depth * 10}
      {@const isSel = $currentSpace?.id === sn.space.id}

      <!-- Space row -->
      <div draggable="true"
        ondragstart={(e) => startDrag(e, sn.space.id, 'space')}
        ondragover={(e) => onSpaceDragOver(e, sn.space.id)}
        ondragleave={onDragLeave}
        ondrop={(e) => onSpaceDrop(e, sn.space.id)}
        class="group flex items-center rounded-xl mb-0.5 transition-all {dragId === sn.space.id ? 'opacity-40' : ''} {dropCls(sn.space.id)}"
        style="padding-left:{si + 4}px; {isSel ? `background:var(--color-surface); box-shadow:var(--shadow-card);` : ''}"
      >
        <!-- Expand arrow -->
        <button onclick={() => { expandedSpaceIds = toggle(expandedSpaceIds, sn.space.id); }}
          class="shrink-0 w-5 h-5 flex items-center justify-center rounded transition-colors mr-0.5"
          style="color:var(--color-on-muted);">
          {#if sn.hasChildren || expandedSpaceIds.has(sn.space.id)}
            {#if expandedSpaceIds.has(sn.space.id)}<ChevronDown size={12} />{:else}<ChevronRight size={12} />{/if}
          {:else}<span class="w-3"></span>{/if}
        </button>

        <!-- Space icon -->
        <div class="shrink-0 w-5 h-5 flex items-center justify-center mr-1.5">
          {#if isSel}<FolderOpen size={14} style="color:var(--color-primary);"/>{:else}<Folder size={14} style="color:var(--color-on-muted);"/>{/if}
        </div>

        <!-- Name / rename input -->
        {#if renamingSpaceId === sn.space.id}
          <input type="text" bind:value={renamingSpaceName}
            onblur={() => commitRenameSpace(sn.space)}
            onkeydown={(e) => { if (e.key === 'Enter') commitRenameSpace(sn.space); else if (e.key === 'Escape') renamingSpaceId = null; }}
            class="flex-1 text-xs font-semibold outline-none rounded px-1 py-2 min-w-0"
            style="background:var(--color-surface-lo); color:var(--color-on-surface); border:1px solid var(--color-primary);"
            use:focusOnMount />
        {:else}
          <button onclick={() => selectSpace(sn.space)} ondblclick={() => startRenameSpace(sn.space)}
            class="flex-1 text-left py-2.5 text-xs font-semibold truncate min-w-0 transition-colors"
            style="color:{isSel ? 'var(--color-primary)' : 'var(--color-on-surface)'};">
            {sn.space.name}
          </button>
        {/if}

        <!-- Remote source indicator (icon only) -->
        {#if sn.space.source !== 'local'}
          <span title="Remote space: {sn.space.server_url ?? sn.space.source}" class="shrink-0 opacity-50" style="color:var(--color-primary);">
            <Cloud size={10} />
          </span>
        {/if}

        <!-- Hover actions -->
        <div class="flex opacity-0 group-hover:opacity-100 transition-all shrink-0 gap-0.5 pr-1">
          <button onclick={() => showNewPageFor(sn.space.id)}
            class="w-6 h-6 flex items-center justify-center rounded-lg transition-colors"
            style="color:var(--color-on-muted);"
            onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
            onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
            title="New page"><Plus size={11} /></button>
          <button onclick={() => { $currentSpace = sn.space; showImport = true; }}
            class="w-6 h-6 flex items-center justify-center rounded-lg transition-colors"
            style="color:var(--color-on-muted);"
            onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
            onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
            title="Import"><UploadCloud size={11} /></button>
          <button onclick={() => showNewSubSpaceFor(sn.space.id)}
            class="w-6 h-6 flex items-center justify-center rounded-lg transition-colors"
            style="color:var(--color-on-muted);"
            onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
            onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
            title="New folder"><Folder size={11} /></button>
          {#if sn.space.source === 'remote'}
            <button onclick={() => showConnectChildFor(sn.space.id)}
              class="w-6 h-6 flex items-center justify-center rounded-lg transition-colors"
              style="color:var(--color-on-muted);"
              onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
              onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
              title="Connect child namespace"><FolderOpen size={11} /></button>
          {/if}
          <button onclick={() => startRenameSpace(sn.space)}
            class="w-6 h-6 flex items-center justify-center rounded-lg transition-colors"
            style="color:var(--color-on-muted);"
            onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
            onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
            title="Rename"><Pencil size={11} /></button>
          <button onclick={() => handleDeleteSpace(sn.space)}
            class="w-6 h-6 flex items-center justify-center rounded-lg transition-colors"
            style="color:var(--color-on-muted);"
            onmouseenter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'; (e.currentTarget as HTMLElement).style.color = '#ef4444'; }}
            onmouseleave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; (e.currentTarget as HTMLElement).style.color = 'var(--color-on-muted)'; }}
            title="Delete space"><X size={11} /></button>
        </div>
      </div>

      <!-- Selected-space action bar -->
      {#if sn.space.id === $currentSpace?.id}
        {@const barIndent = si + 24}
        <div class="flex items-center gap-1 mb-1" style="padding-left:{barIndent}px">
          <button onclick={() => handleProcessSpace(sn.space.id)}
            class="flex items-center gap-1 text-xs px-2 py-0.5 rounded-md transition-colors"
            style="color:var(--color-on-muted); background:var(--color-surface-lo);"
            onmouseenter={(e) => (e.currentTarget as HTMLElement).style.color = 'var(--color-primary)'}
            onmouseleave={(e) => (e.currentTarget as HTMLElement).style.color = 'var(--color-on-muted)'}
            title="Analyze and index all pages">
            <Sparkles size={10} />Process all
          </button>
        </div>
      {/if}

      <!-- Per-space inline forms (shown regardless of selection) -->
      {#if newPageForSpace[sn.space.id]}
        {@const formIndent = si + 28}
        <div class="flex items-center gap-1 pr-2 py-1 mb-1" style="padding-left:{formIndent}px">
          <input type="text"
            placeholder="Page title"
            onkeyup={(e) => {
              const inp = e.currentTarget as HTMLInputElement;
              if (e.key === 'Enter') { handleCreatePageForSpace(sn.space.id, inp.value); inp.value = ''; }
              else if (e.key === 'Escape') hideNewPageFor(sn.space.id);
            }}
            class="flex-1 text-xs px-2 py-1.5 rounded-lg outline-none"
            style="background:var(--color-surface-lo); color:var(--color-on-surface); border:1px solid var(--color-primary);"
            use:focusOnMount />
          <button onclick={(e) => {
            const inp = (e.currentTarget as HTMLElement).previousElementSibling as HTMLInputElement;
            handleCreatePageForSpace(sn.space.id, inp.value);
          }} class="text-xs px-2 py-1 rounded-lg font-semibold text-white" style="background:var(--color-primary);">✓</button>
          <button onclick={() => hideNewPageFor(sn.space.id)} class="text-xs px-1.5 py-1" style="color:var(--color-on-muted);">✕</button>
        </div>
      {/if}

      {#if newSubSpaceFor[sn.space.id]}
        {@const formIndent = si + 20}
        <div class="flex flex-col gap-0.5 pr-2 py-1 mb-1" style="padding-left:{formIndent}px">
          <div class="flex items-center gap-1">
            <input type="text"
              bind:value={newSubSpaceFor[sn.space.id].name}
              placeholder="Folder name"
              onkeyup={(e) => { if (e.key === 'Enter') handleCreateSubSpaceFor(sn.space.id); else if (e.key === 'Escape') hideNewSubSpaceFor(sn.space.id); }}
              oninput={() => { newSubSpaceFor = { ...newSubSpaceFor, [sn.space.id]: { ...newSubSpaceFor[sn.space.id], error: '' } }; }}
              class="flex-1 text-xs px-2 py-1.5 rounded-lg outline-none"
              style="background:var(--color-surface-lo); color:var(--color-on-surface); border:1px solid {newSubSpaceFor[sn.space.id].error ? '#ef4444' : 'var(--color-primary)'};"
              use:focusOnMount />
            <button onclick={() => handleCreateSubSpaceFor(sn.space.id)} class="text-xs px-2 py-1 rounded-lg font-semibold text-white" style="background:var(--color-primary);">✓</button>
            <button onclick={() => hideNewSubSpaceFor(sn.space.id)} class="text-xs px-1.5 py-1" style="color:var(--color-on-muted);">✕</button>
          </div>
          {#if newSubSpaceFor[sn.space.id].error}
            <p class="text-[10px] px-1" style="color:#ef4444;">{newSubSpaceFor[sn.space.id].error}</p>
          {/if}
        </div>
      {/if}

      {#if connectChildFor[sn.space.id]}
        {@const entry = connectChildFor[sn.space.id]}
        {@const formIndent = si + 20}
        <div class="flex flex-col gap-1 pr-2 py-2 mb-1" style="padding-left:{formIndent}px">
          <input type="text"
            bind:value={entry.url}
            placeholder="Server URL (http://127.0.0.1:8095)"
            class="text-xs px-2 py-1.5 rounded-lg outline-none"
            style="background:var(--color-surface-lo); color:var(--color-on-surface); border:1px solid var(--color-border);" />
          <input type="text"
            bind:value={entry.name}
            placeholder="Name (optional)"
            class="text-xs px-2 py-1.5 rounded-lg outline-none"
            style="background:var(--color-surface-lo); color:var(--color-on-surface); border:1px solid var(--color-border);" />
          <input type="password"
            bind:value={entry.token}
            placeholder="Token (optional)"
            class="text-xs px-2 py-1.5 rounded-lg outline-none"
            style="background:var(--color-surface-lo); color:var(--color-on-surface); border:1px solid var(--color-border);" />
          <select bind:value={entry.permission}
            class="text-xs px-2 py-1.5 rounded-lg outline-none"
            style="background:var(--color-surface-lo); color:var(--color-on-surface); border:1px solid var(--color-border);">
            <option value="read">read</option>
            <option value="write">write</option>
            <option value="owner">owner</option>
          </select>
          <div class="flex gap-1">
            <button onclick={() => handleConnectChildSpace(sn.space.id)}
              disabled={entry.connecting || !entry.url.trim()}
              class="flex-1 text-xs py-1 rounded-lg font-semibold text-white disabled:opacity-40 flex items-center justify-center gap-1"
              style="background:var(--color-primary);">
              {#if entry.connecting}<span class="animate-spin w-3 h-3 border border-white border-t-transparent rounded-full inline-block"></span>{/if}
              Connect
            </button>
            <button onclick={() => hideConnectChildFor(sn.space.id)} class="text-xs px-2 py-1 rounded-lg" style="color:var(--color-on-muted); border:1px solid var(--color-border);">✕</button>
          </div>
        </div>
      {/if}

      <!-- Page tree under selected space -->
      {#if isSel}
        {@const indent = si + 24}

        <!-- Page list -->
        {#if _pageNodes.length === 0 && $pages.length > 0}
          <p class="text-xs px-2 py-1" style="color:var(--color-on-muted); opacity:0.5;">Pages loaded ({$pages.length}) but not rendering — report this bug</p>
        {/if}
        {#each _pageNodes as pn (pn.page.id)}
          {@const pi = pn.depth * 10 + indent}
          <div draggable="true"
            ondragstart={(e) => startDrag(e, pn.page.id, 'page')}
            ondragover={(e) => onPageDragOver(e, pn.page.id)}
            ondragleave={onDragLeave}
            ondrop={(e) => onPageDrop(e, pn.page.id)}
            class="group flex items-center rounded-lg mb-0.5 transition-all {dragId === pn.page.id ? 'opacity-40' : ''} {dropCls(pn.page.id)}"
            style="padding-left:{pi}px;"
          >
            <button onclick={() => { expandedIds = toggle(expandedIds, pn.page.id); }}
              class="shrink-0 w-4 h-6 flex items-center justify-center mr-0.5"
              style="color:var(--color-on-muted);">
              {#if pn.hasChildren || expandedIds.has(pn.page.id)}
                {#if expandedIds.has(pn.page.id)}<ChevronDown size={10} />{:else}<ChevronRight size={10} />{/if}
              {:else}<span class="w-2"></span>{/if}
            </button>

            <FileText size={11} class="shrink-0 mr-1.5" style="color:{$currentPage?.id === pn.page.id ? 'var(--color-primary)' : 'var(--color-on-muted)'};" />

            <button onclick={() => selectPage(pn.page)}
              class="flex-1 text-left py-1.5 text-xs truncate min-w-0 transition-colors font-medium"
              style="color:{$currentPage?.id === pn.page.id ? 'var(--color-primary)' : 'var(--color-on-surface)'};">
              {pn.page.title}
            </button>

            <!-- Read-only badge -->
            {#if pn.page.permission_level === 'read'}
              <span title="Read-only (synced from {pn.page.source})" class="shrink-0 opacity-60">
                <Lock size={9} style="color:var(--color-on-muted);" />
              </span>
            {/if}

            <div class="flex opacity-0 group-hover:opacity-100 transition-all shrink-0 gap-0.5 pr-1">
              <button onclick={() => showNewChild(pn.page.id)}
                class="w-5 h-5 flex items-center justify-center rounded text-xs"
                style="color:var(--color-on-muted);"
                onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
                onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
                title="Sub-page"><Plus size={10} /></button>
              <button onclick={() => { moveModalPage = pn.page; moveTargetId = ''; }}
                class="w-5 h-5 flex items-center justify-center rounded text-xs"
                style="color:var(--color-on-muted);"
                onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
                onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
                title="Move to space"><MoveRight size={10} /></button>
              <button onclick={() => handleDeletePage(pn.page)}
                class="w-5 h-5 flex items-center justify-center rounded text-xs"
                style="color:var(--color-on-muted);"
                onmouseenter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'; (e.currentTarget as HTMLElement).style.color = '#ef4444'; }}
                onmouseleave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; (e.currentTarget as HTMLElement).style.color = 'var(--color-on-muted)'; }}
                title="Move to trash"><X size={10} /></button>
            </div>
          </div>

          {#if newChildState[pn.page.id]?.show}
            <div class="flex items-center gap-1 pr-2 py-0.5" style="padding-left:{pi + 16}px">
              <input type="text" bind:value={newChildState[pn.page.id].title} placeholder="Sub-page title"
                onkeyup={(e) => { if (e.key === 'Enter') handleCreateChildPage(pn.page.id); else if (e.key === 'Escape') hideNewChild(pn.page.id); }}
                class="flex-1 text-xs px-2 py-1 rounded-lg outline-none"
                style="background:var(--color-surface-lo); color:var(--color-on-surface); border:1px solid var(--color-primary);"
                use:focusOnMount />
              <button onclick={() => handleCreateChildPage(pn.page.id)} class="text-xs px-2 py-1 rounded-lg font-semibold text-white" style="background:var(--color-primary);">✓</button>
            </div>
          {/if}
        {/each}
      {/if}
    {/each}

    <!-- New root space form -->
    {#if showNewSpace}
      <div class="flex items-center gap-1 px-2 py-2">
        <input type="text" bind:value={newSpaceName} placeholder="Space name"
          onkeyup={(e) => { if (e.key === 'Enter') handleCreateSpace(); else if (e.key === 'Escape') { showNewSpace = false; newSpaceName = ''; } }}
          class="flex-1 text-xs px-2.5 py-1.5 rounded-lg outline-none"
          style="background:var(--color-surface-lo); color:var(--color-on-surface); border:1px solid var(--color-primary);"
          use:focusOnMount />
        <button onclick={handleCreateSpace} class="text-xs px-2 py-1 rounded-lg font-semibold text-white" style="background:var(--color-primary);">✓</button>
        <button onclick={() => { showNewSpace = false; newSpaceName = ''; }} class="text-xs px-1.5 py-1" style="color:var(--color-on-muted);">✕</button>
      </div>
    {:else if !$currentSpace}
      <!-- shown via gradient button above when no space selected, but also inline link -->
    {:else}
      <!-- Additional "New Space" link at bottom of list -->
      <button onclick={() => showNewSpace = true}
        class="w-full text-left px-3 py-2 text-xs rounded-lg transition-colors flex items-center gap-1.5 mt-1"
        style="color:var(--color-on-muted);"
        onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
        onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
      >
        <Plus size={11} /> New space
      </button>
    {/if}

  </div>

  <!-- ── Trash ─────────────────────────────────────────────────────────────── -->
  {#if $currentSpace}
    <div class="flex-shrink-0 border-t" style="border-color:var(--color-border);">
      <button onclick={toggleTrash}
        class="w-full flex items-center gap-2.5 px-4 py-2.5 text-xs font-medium transition-colors"
        style="color:{showTrash ? '#ef4444' : 'var(--color-on-muted)'}; background:{showTrash ? 'rgba(239,68,68,0.06)' : 'transparent'};"
        onmouseenter={(e) => { if (!showTrash) (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'; }}
        onmouseleave={(e) => { if (!showTrash) (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
      >
        <Trash2 size={13} />
        <span>Trash</span>
        <span class="ml-auto">{#if showTrash}<ChevronDown size={11} />{:else}<ChevronRight size={11} />{/if}</span>
      </button>
      {#if showTrash}
        <div class="max-h-36 overflow-y-auto border-t" style="border-color:var(--color-border);">
          {#if trashPages.length === 0}
            <p class="text-xs px-4 py-2.5" style="color:var(--color-on-muted); opacity:0.6;">Trash is empty</p>
          {:else}
            {#each trashPages as tp (tp.id)}
              <div class="flex items-center group px-4 py-1.5 transition-colors"
                onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
                onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
              >
                <span class="flex-1 text-xs truncate line-through min-w-0" style="color:var(--color-on-muted);">{tp.title}</span>
                <div class="flex opacity-0 group-hover:opacity-100 shrink-0 gap-1 ml-1">
                  <button onclick={() => handleRestore(tp)} class="text-xs px-1.5 py-0.5 rounded transition-colors" style="color:#22c55e;" title="Restore">↺</button>
                  <button onclick={() => handlePermanentDelete(tp)} class="text-xs px-1.5 py-0.5 rounded transition-colors" style="color:#ef4444;" title="Delete forever">✕</button>
                </div>
              </div>
            {/each}
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  <!-- ── User + action bar ──────────────────────────────────────────────────── -->
  <div class="flex-shrink-0 border-t px-3 py-3" style="border-color:var(--color-border);">
    <div class="flex items-center gap-2.5">
      <!-- Avatar -->
      <div class="w-7 h-7 rounded-full flex items-center justify-center text-xs font-bold text-white flex-shrink-0"
        style="background:linear-gradient(135deg, var(--color-primary), var(--color-primary-dim));">
        {initial($userName)}
      </div>
      <!-- Name -->
      <div class="flex-1 min-w-0">
        <p class="text-xs font-semibold truncate text-on-surface">{$userName || 'You'}</p>
      </div>
      <!-- Action icons -->
      <div class="flex gap-0.5">
        <button onclick={() => $currentSpace && (showImport = true)} disabled={!$currentSpace}
          class="w-7 h-7 flex items-center justify-center rounded-lg transition-colors"
          style="color:var(--color-on-muted);"
          onmouseenter={(e) => { if ($currentSpace) (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'; }}
          onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
          title="Import"><UploadCloud size={13} /></button>
        <button onclick={handleSync}
          class="w-7 h-7 flex items-center justify-center rounded-lg transition-colors"
          style="color:var(--color-on-muted);"
          onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
          onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
          title="Sync"><RefreshCw size={13} class={syncing ? 'animate-spin' : ''} /></button>
        <button onclick={() => theme.toggle()}
          class="w-7 h-7 flex items-center justify-center rounded-lg transition-colors"
          style="color:var(--color-on-muted);"
          onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
          onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
          title="{$theme === 'dark' ? 'Light mode' : 'Dark mode'}">
          {#if $theme === 'dark'}<Sun size={13} />{:else}<Moon size={13} />{/if}
        </button>
        <button onclick={() => showSettings = true}
          class="w-7 h-7 flex items-center justify-center rounded-lg transition-colors"
          style="color:var(--color-on-muted);"
          onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--color-surface-lo)'}
          onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
          title="Settings"><Settings size={13} /></button>
      </div>
    </div>
  </div>

  <ActivityPanel />
</aside>

{#if showImport}
  <ImportModal onclose={() => showImport = false} />
{/if}
{#if showSettings}
  <SettingsModal onclose={() => showSettings = false} />
{/if}

{#if moveModalPage}
  <!-- Move to space modal -->
  <div class="fixed inset-0 z-50 flex items-center justify-center" style="background:rgba(0,0,0,0.5);">
    <div class="rounded-2xl shadow-xl p-6 w-80 flex flex-col gap-4" style="background:var(--color-surface); border:1px solid var(--color-border);">
      <div>
        <div class="font-semibold text-sm mb-1" style="color:var(--color-on-surface);">Move to space</div>
        <div class="text-xs truncate" style="color:var(--color-on-muted);">"{moveModalPage.title}" and all its sub-pages</div>
      </div>
      <select
        bind:value={moveTargetId}
        class="w-full text-sm px-3 py-2 rounded-lg outline-none"
        style="background:var(--color-surface-lo); color:var(--color-on-surface); border:1px solid var(--color-border);"
      >
        <option value="">— choose destination —</option>
        {#each $spaces.filter(s => s.id !== $currentSpace?.id) as s}
          <option value={s.id}>{s.name}</option>
        {/each}
      </select>
      <div class="flex gap-2 justify-end">
        <button
          onclick={() => { moveModalPage = null; moveTargetId = ''; }}
          class="px-3 py-1.5 text-xs rounded-lg"
          style="background:var(--color-surface-lo); color:var(--color-on-muted);">Cancel</button>
        <button
          onclick={confirmMoveToSpace}
          disabled={!moveTargetId}
          class="px-3 py-1.5 text-xs rounded-lg font-semibold text-white disabled:opacity-40"
          style="background:var(--color-primary);">Move</button>
      </div>
    </div>
  </div>
{/if}
