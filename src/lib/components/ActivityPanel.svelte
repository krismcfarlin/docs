<script lang="ts">
  import { activityLog } from '$lib/stores';

  let collapsed = $state(false);

  function timeAgo(ts: number): string {
    const s = Math.floor((Date.now() - ts) / 1000);
    if (s < 60) return `${s}s ago`;
    if (s < 3600) return `${Math.floor(s / 60)}m ago`;
    return `${Math.floor(s / 3600)}h ago`;
  }

  const icons: Record<string, string> = {
    running: '⟳',
    done:    '✓',
    error:   '✕',
  };

  const colors: Record<string, string> = {
    running: 'text-slate-400',
    done:    'text-green-500',
    error:   'text-red-500',
  };
</script>

{#if $activityLog.length > 0}
  <div class="border-t border-slate-800 bg-slate-950/60">
    <!-- Header row -->
    <button
      onclick={() => collapsed = !collapsed}
      class="w-full flex items-center justify-between px-4 py-1.5 text-xs text-slate-500 hover:text-slate-400 transition-colors"
    >
      <span class="font-medium tracking-wide uppercase">Activity</span>
      <span>{collapsed ? '▲' : '▼'}</span>
    </button>

    {#if !collapsed}
      <div class="max-h-40 overflow-y-auto pb-1">
        {#each $activityLog as event (event.id)}
          <div class="flex items-start gap-2 px-4 py-0.5">
            <span class="shrink-0 mt-px {colors[event.status]} {event.status === 'running' ? 'animate-spin' : ''}">
              {icons[event.status]}
            </span>
            <div class="min-w-0 flex-1">
              <span class="text-xs text-slate-400 truncate block">{event.label}</span>
              {#if event.detail}
                <span class="text-xs text-slate-600 truncate block">{event.detail}</span>
              {/if}
            </div>
            <span class="text-xs text-slate-700 shrink-0 tabular-nums">{timeAgo(event.ts)}</span>
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}
