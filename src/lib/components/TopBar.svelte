<script lang="ts">
  import { currentPage, currentVersion, versions, pages, readMode, activityStart, activityDone, activityError, currentSpace, lastSynthesisAt } from '$lib/stores';
  import { publishVersion, freezeVersion, forkVersion, listPageVersions, savePageVersion, getPageVersion, vectorizePage, renamePage, synthesizePage, demoteEntityPage } from '$lib/api';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { save as saveDialog } from '@tauri-apps/plugin-dialog';
  import DiffModal from './DiffModal.svelte';
  import { BookOpen, Pencil, GitFork, Globe, GitCompare, Download, ChevronDown, Sparkles, Trash2 } from 'lucide-svelte';

  let synthesizing = $state(false);

  async function handleSynthesize() {
    if (!$currentPage || !$currentSpace || $currentSpace.source !== 'remote') return;
    synthesizing = true;
    const pageId = $currentPage.id;
    const id = activityStart(`Process "${$currentPage.title}"`);

    // Track per-stage progress events from Rust
    let stageActivityId: string | null = null;
    const unlisten = await listen<{ page_id: string; stage: string; label: string }>('synthesis:stage', (event) => {
      const { page_id, stage, label } = event.payload;
      if (page_id !== pageId) return;
      if (stageActivityId) {
        if (stage === 'done') {
          activityDone(stageActivityId, label);
          stageActivityId = null;
        } else {
          activityDone(stageActivityId, label);
          stageActivityId = activityStart(label);
        }
      } else {
        stageActivityId = activityStart(label);
      }
    });

    try {
      await synthesizePage($currentSpace.id, pageId);
      $lastSynthesisAt = Date.now();
      if (stageActivityId) { activityDone(stageActivityId, 'Done'); stageActivityId = null; }
      // Also vectorize so page is searchable
      if ($currentVersion) {
        await vectorizePage($currentVersion.id, $currentPage!.space_id).catch(() => {});
      }
      activityDone(id, 'Done');
    } catch (e) {
      if (stageActivityId) { activityError(stageActivityId, String(e)); stageActivityId = null; }
      activityError(id, String(e));
    } finally {
      unlisten();
      synthesizing = false;
    }
  }

  let showVersionPicker = $state(false);
  let showDiff          = $state(false);
  let showExportMenu    = $state(false);

  async function exportMarkdown() {
    showExportMenu = false;
    if (!$currentVersion?.content) return;
    const defaultName = `${($currentVersion.title ?? $currentPage?.title ?? 'page').replace(/[^a-z0-9]/gi, '_')}.md`;
    const path = await saveDialog({
      defaultPath: defaultName,
      filters: [{ name: 'Markdown', extensions: ['md'] }],
    });
    if (path) {
      const id = activityStart(`Export "${$currentVersion.title ?? 'Untitled'}"`);
      try {
        await invoke('write_text_file', { path, content: $currentVersion.content });
        activityDone(id, path);
      } catch (e) {
        activityError(id, String(e));
      }
    }
  }

  async function exportPdf() {
    showExportMenu = false;
    try {
      await getCurrentWebview().print();
    } catch {
      window.print();
    }
  }

  async function handlePublish() {
    if (!$currentVersion) return;
    const id = activityStart(`Publish "${$currentVersion.title ?? 'Untitled'}"`);
    try {
      await publishVersion($currentVersion.id, $currentPage!.space_id);
      $currentVersion = { ...$currentVersion, is_published: true };
      activityDone(id);
    } catch (e) {
      activityError(id, String(e));
    }
  }

  async function handleFork() {
    if (!$currentVersion) return;
    const newVer = await forkVersion($currentVersion.id, $currentPage!.space_id);
    $currentVersion = newVer;
    if ($currentPage) {
      $versions = await listPageVersions($currentPage.id, $currentPage.space_id);
    }
  }

  async function selectVersion(ver: typeof $versions[0]) {
    showVersionPicker = false;
    const full = await getPageVersion(ver.page_id, $currentPage!.space_id, ver.id);
    if (full) $currentVersion = full;
  }

  function versionLabel(ver: typeof $versions[0]) {
    const parts = [`v${ver.version_num}`];
    if (ver.is_published) parts.push('Published');
    else parts.push('Draft');
    return parts.join(' · ');
  }

  async function handleDemote(): Promise<void> {
    if (!$currentPage || !$currentSpace) return;
    const id = activityStart(`Demoting: ${$currentPage.title}…`);
    try {
      await demoteEntityPage($currentSpace.id, $currentPage.id);
      $currentPage = null;
      $currentVersion = null;
      activityDone(id, 'Wiki page removed');
    } catch (e) {
      activityError(id, String(e));
    }
  }
</script>

{#if $currentPage && $currentVersion}
  <div
    class="flex items-center gap-2 px-5 py-2.5"
    style="background: var(--color-surface); border-bottom: 1px solid var(--color-border);"
    data-topbar
  >
    <!-- Title (editable) -->
    <input
      type="text"
      value={$currentVersion.title ?? $currentPage.title}
      onblur={async (e) => {
        if (!$currentVersion || !$currentPage) return;
        const newTitle = (e.currentTarget as HTMLInputElement).value.trim() || 'Untitled';
        const pageId = $currentPage.id;
        const spaceId = $currentPage.space_id;
        const content = $currentVersion.content ?? '';
        const textContent = $currentVersion.text_content ?? '';
        // Update store immediately so sidebar reflects change
        $currentVersion = { ...$currentVersion, title: newTitle };
        $currentPage = { ...$currentPage, title: newTitle };
        $pages = $pages.map(p => p.id === pageId ? { ...p, title: newTitle } : p);
        // Persist in background
        Promise.all([
          renamePage(pageId, newTitle, spaceId),
          savePageVersion($currentVersion.id, newTitle, content, textContent, spaceId, $currentVersion.updated_at ?? ''),
        ]).catch(e => console.error('[rename] failed to persist:', e));
      }}
      class="flex-1 bg-transparent text-base font-semibold outline-none min-w-0 truncate transition-colors"
      style="color: var(--color-on-surface); border-bottom: 1px solid transparent;"
      placeholder="Untitled"
    />

    <!-- Version picker -->
    <div class="relative">
      <button
        onclick={() => showVersionPicker = !showVersionPicker}
        class="flex items-center gap-1 text-xs px-2.5 py-1 rounded-full font-mono transition-colors"
        style="border: 1px solid var(--color-border); color: var(--color-on-muted); background: var(--color-surface-lo);"
      >
        v{$currentVersion.version_num}
        {#if $versions.length > 1}
          <ChevronDown size={10} />
        {/if}
      </button>

      {#if showVersionPicker && $versions.length > 1}
        <div
          class="fixed inset-0 z-40"
          role="button"
          tabindex="-1"
          onclick={() => showVersionPicker = false}
          onkeydown={() => {}}
        ></div>
        <div
          class="absolute right-0 top-full mt-1 z-50 rounded-xl shadow-ambient min-w-52 py-1 overflow-hidden"
          style="background: var(--color-surface); border: 1px solid var(--color-border);"
        >
          {#each $versions as ver}
            <button
              onclick={() => selectVersion(ver)}
              class="w-full text-left px-3 py-2 text-xs transition-colors flex items-center justify-between gap-3"
              style={ver.id === $currentVersion.id
                ? 'background: var(--color-surface-lo); color: var(--color-on-surface);'
                : 'color: var(--color-on-muted);'}
            >
              <span class="font-mono">{versionLabel(ver)}</span>
              <span style="color: var(--color-on-muted); opacity: 0.5;">{ver.updated_at.slice(0, 10)}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Status badge -->
    {#if $currentVersion.is_published}
      <span class="text-xs px-2.5 py-0.5 rounded-full font-medium text-green-600 dark:text-green-400"
        style="background: var(--color-surface-lo); border: 1px solid currentColor; opacity: 0.9;">Published</span>
    {:else}
      <span class="text-xs px-2.5 py-0.5 rounded-full font-medium"
        style="background: var(--color-surface-lo); color: var(--color-on-muted);">Draft</span>
    {/if}

    <!-- Divider -->
    <div class="w-px h-4 mx-1" style="background: var(--color-border);"></div>

    <!-- Actions -->
    <div class="flex items-center gap-0.5">
      <!-- Read / Edit mode toggle -->
      <button
        onclick={() => $readMode = !$readMode}
        class="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg transition-colors"
        style={$readMode
          ? 'background: #fef3c7; color: #b45309;'
          : 'color: var(--color-on-muted);'}
        title={$readMode ? 'Switch to edit mode' : 'Switch to read mode'}
      >
        {#if $readMode}
          <Pencil size={12} />
          Edit
        {:else}
          <BookOpen size={12} />
          Read
        {/if}
      </button>

      <button
        onclick={handleFork}
        class="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg transition-colors"
        style="color: var(--color-on-muted);"
        title="Fork version"
      >
        <GitFork size={12} />
        Fork
      </button>

      <button
        onclick={handlePublish}
        disabled={$currentVersion.is_published}
        class="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        style="color: var(--color-on-muted);"
        title="Publish version"
      >
        <Globe size={12} />
        Publish
      </button>

      {#if $versions.length > 1}
        <button
          onclick={() => showDiff = true}
          class="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg transition-colors"
          style="color: var(--color-on-muted);"
          title="Compare versions"
        >
          <GitCompare size={12} />
          Diff
        </button>
      {/if}

      {#if $currentSpace?.source === 'remote'}
        <button
          onclick={handleSynthesize}
          disabled={synthesizing}
          class="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg transition-colors disabled:opacity-40"
          style="color: var(--color-on-muted);"
          title="Process this document with AI"
        >
          <Sparkles size={12} />
          {synthesizing ? 'Processing…' : 'Process'}
        </button>
      {/if}

      {#if $currentPage?.is_entity_page === 1}
        <button
          onclick={handleDemote}
          class="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg transition-colors"
          style="color: #dc2626;"
          title="Remove this wiki page and reset the entity"
        >
          <Trash2 size={12} />
          Demote
        </button>
      {/if}

      <!-- Export dropdown -->
      <div class="relative">
        <button
          onclick={() => showExportMenu = !showExportMenu}
          class="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg transition-colors"
          style="color: var(--color-on-muted);"
          title="Export"
        >
          <Download size={12} />
          Export
          <ChevronDown size={10} />
        </button>

        {#if showExportMenu}
          <div
            class="fixed inset-0 z-40"
            role="button"
            tabindex="-1"
            onclick={() => showExportMenu = false}
            onkeydown={() => {}}
          ></div>
          <div
            class="absolute right-0 top-full mt-1 z-50 rounded-xl shadow-ambient min-w-40 py-1 overflow-hidden"
            style="background: var(--color-surface); border: 1px solid var(--color-border);"
          >
            <button
              onclick={exportMarkdown}
              class="w-full text-left px-3 py-2 text-xs transition-colors"
              style="color: var(--color-on-muted);"
            >Download .md</button>
            <button
              onclick={exportPdf}
              class="w-full text-left px-3 py-2 text-xs transition-colors"
              style="color: var(--color-on-muted);"
            >Save as PDF…</button>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

{#if showDiff}
  <DiffModal onclose={() => showDiff = false} />
{/if}
