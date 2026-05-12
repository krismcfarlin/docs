<script lang="ts">
  import { diffLines, type Change } from 'diff';
  import { versions, currentVersion } from '$lib/stores';
  import { getPageVersion } from '$lib/api';

  let { onclose }: { onclose: () => void } = $props();

  let leftId  = $state($currentVersion?.id ?? '');
  let rightId = $state('');
  let changes = $state<Change[]>([]);
  let loading = $state(false);
  let error   = $state('');

  // Default right to the version just before current
  $effect(() => {
    if ($versions.length > 1 && !rightId) {
      const cur = $versions.findIndex(v => v.id === leftId);
      const other = cur === 0 ? 1 : cur - 1;
      rightId = $versions[other]?.id ?? '';
    }
  });

  $effect(() => {
    if (leftId && rightId) runDiff();
  });

  async function runDiff() {
    if (!leftId || !rightId || leftId === rightId) { changes = []; return; }
    loading = true;
    error = '';
    try {
      const pageId = $versions[0]?.page_id ?? $currentVersion?.page_id ?? '';
      const [l, r] = await Promise.all([
        getPageVersion(pageId, leftId),
        getPageVersion(pageId, rightId),
      ]);
      const lText = l?.content ?? '';
      const rText = r?.content ?? '';
      changes = diffLines(lText, rText);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function versionLabel(id: string) {
    const v = $versions.find(v => v.id === id);
    if (!v) return id.slice(0, 8);
    const parts = [`v${v.version_num}`];
    if (v.is_frozen) parts.push('Frozen');
    else if (v.is_published) parts.push('Published');
    else parts.push('Draft');
    return parts.join(' · ');
  }

  const added   = (c: Change) => c.added   === true;
  const removed = (c: Change) => c.removed === true;
</script>

<svelte:window onkeydown={(e) => { if (e.key === 'Escape') onclose(); }} />

<div
  class="fixed inset-0 z-50 bg-black/70 flex items-center justify-center"
  role="button"
  tabindex="-1"
  onclick={onclose}
  onkeydown={() => {}}
>
  <div
    class="bg-slate-900 border border-slate-700 rounded-xl shadow-2xl w-full max-w-4xl mx-4 flex flex-col"
    style="height: 80vh"
    role="dialog"
    onclick={(e) => e.stopPropagation()}
    onkeydown={() => {}}
  >
    <!-- Header -->
    <div class="flex items-center justify-between px-5 py-4 border-b border-slate-700 shrink-0">
      <h2 class="text-slate-200 font-semibold">Compare versions</h2>
      <button onclick={onclose} class="text-slate-500 hover:text-slate-300 text-lg">✕</button>
    </div>

    <!-- Version selectors -->
    <div class="flex items-center gap-3 px-5 py-3 border-b border-slate-700 shrink-0">
      <div class="flex-1">
        <label class="text-xs text-slate-500 block mb-1">Base</label>
        <select
          bind:value={leftId}
          class="w-full bg-slate-800 text-slate-200 text-sm px-3 py-1.5 rounded border border-slate-700 outline-none"
        >
          {#each $versions as v}
            <option value={v.id}>{versionLabel(v.id)} — {v.updated_at.slice(0, 10)}</option>
          {/each}
        </select>
      </div>

      <span class="text-slate-600 mt-5">→</span>

      <div class="flex-1">
        <label class="text-xs text-slate-500 block mb-1">Compare</label>
        <select
          bind:value={rightId}
          class="w-full bg-slate-800 text-slate-200 text-sm px-3 py-1.5 rounded border border-slate-700 outline-none"
        >
          {#each $versions as v}
            <option value={v.id}>{versionLabel(v.id)} — {v.updated_at.slice(0, 10)}</option>
          {/each}
        </select>
      </div>

      <!-- Stats -->
      {#if changes.length > 0}
        {@const added_lines   = changes.filter(c => c.added).reduce((n, c) => n + (c.count ?? 0), 0)}
        {@const removed_lines = changes.filter(c => c.removed).reduce((n, c) => n + (c.count ?? 0), 0)}
        <div class="flex gap-3 mt-5 text-xs font-mono shrink-0">
          <span class="text-green-500">+{added_lines}</span>
          <span class="text-red-500">-{removed_lines}</span>
        </div>
      {/if}
    </div>

    <!-- Diff output -->
    <div class="flex-1 overflow-y-auto font-mono text-xs p-4 space-y-0">
      {#if loading}
        <p class="text-slate-500 p-4">Computing diff…</p>
      {:else if error}
        <p class="text-red-400 p-4">{error}</p>
      {:else if leftId === rightId}
        <p class="text-slate-500 p-4">Select two different versions to compare.</p>
      {:else if changes.length === 0}
        <p class="text-slate-500 p-4">No differences.</p>
      {:else}
        {#each changes as chunk}
          {#each chunk.value.split('\n') as line, i}
            {#if !(i === chunk.value.split('\n').length - 1 && line === '')}
              <div class="flex leading-5 {added(chunk) ? 'bg-green-900/30' : removed(chunk) ? 'bg-red-900/30' : ''}">
                <span class="w-5 shrink-0 select-none {added(chunk) ? 'text-green-500' : removed(chunk) ? 'text-red-500' : 'text-slate-700'}">
                  {added(chunk) ? '+' : removed(chunk) ? '−' : ' '}
                </span>
                <span class="{added(chunk) ? 'text-green-300' : removed(chunk) ? 'text-red-300' : 'text-slate-400'} whitespace-pre-wrap break-all">
                  {line}
                </span>
              </div>
            {/if}
          {/each}
        {/each}
      {/if}
    </div>
  </div>
</div>
