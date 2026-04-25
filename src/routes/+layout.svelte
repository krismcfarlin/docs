<script lang="ts">
  import '../app.css';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import FirstLaunchModal from '$lib/components/FirstLaunchModal.svelte';
  import { spaces, currentSpace, pages, theme, searchFocusTick, userName, userEmail } from '$lib/stores';
  import { initDb, getSpaces, getSettings, getPages, getRecentPages, getPageSynthesis, synthesizePage, getPageVersion, freezeVersion, vectorizePage } from '$lib/api';
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import { get } from 'svelte/store';

  let { children } = $props();

  $effect(() => {
    if (browser) {
      const html = document.documentElement;
      html.classList.remove('dark', 'light');
      html.classList.add($theme);
    }
  });

  let autoQueueTimer: ReturnType<typeof setInterval>;

  async function runAutoQueue() {
    if (!browser) return;
    const TEN_MIN_MS = 10 * 60 * 1000;
    const now = Date.now();
    const remoteSpaces = get(spaces).filter(s => s.source === 'remote');
    for (const space of remoteSpaces) {
      try {
        const pageList = await getPages(space.id);
        for (const page of pageList) {
          const updatedMs = page.updated_at ? new Date(page.updated_at).getTime() : 0;
          if (now - updatedMs < TEN_MIN_MS) continue;
          // Auto-analyze if missing or stale
          try {
            const synth = await getPageSynthesis(space.id, page.id).catch(() => null);
            if (!synth || (page.updated_at && synth.synthesized_at < page.updated_at)) {
              await synthesizePage(space.id, page.id);
            }
          } catch {}
          // Auto-freeze if not frozen
          try {
            const ver = await getPageVersion(page.id, space.id);
            if (ver && !ver.is_frozen) {
              await freezeVersion(ver.id, space.id);
              await vectorizePage(ver.id, space.id);
            }
          } catch {}
        }
      } catch {}
    }
  }

  onMount(async () => {
    try {
      await initDb();
      const [spacesData, settings] = await Promise.all([getSpaces(), getSettings()]);
      $spaces = spacesData;
      if (!$userName && settings.user_name) {
        $userName = settings.user_name;
      }
      $userEmail = settings.user_email ?? '';

      // Auto-select last used space so pages show immediately on launch
      if (spacesData.length > 0) {
        try {
          const recent = await getRecentPages(1);
          const lastSpaceId = recent[0]?.space_id;
          const autoSpace = lastSpaceId
            ? (spacesData.find(s => s.id === lastSpaceId) ?? spacesData[0])
            : spacesData[0];
          $currentSpace = autoSpace;
          $pages = await getPages(autoSpace.id);
        } catch {}
      }
    } catch (e) {
      console.error('DB init failed:', e);
      alert(`DB init failed: ${e}`);
    }
    // Start auto-queue: check every 2 minutes, process pages idle 10+ minutes
    autoQueueTimer = setInterval(runAutoQueue, 2 * 60 * 1000);
  });

  onDestroy(() => {
    clearInterval(autoQueueTimer);
  });

  function onKeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault();
      $searchFocusTick = Date.now();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<FirstLaunchModal />

<div class="flex h-screen overflow-hidden" style="background: var(--color-surface);">
  <Sidebar />
  <main class="flex flex-col flex-1 min-w-0 overflow-hidden" style="background: var(--color-surface);">
    {@render children()}
  </main>
</div>
