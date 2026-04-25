<script lang="ts">
  import { currentVersion } from '$lib/stores';
</script>

<div class="flex-1 flex items-center justify-center bg-surface">
  <div class="text-center text-slate-600">
    <div class="text-4xl mb-4">✦</div>
    <p class="text-sm">Editor coming next sprint</p>
    {#if $currentVersion?.content}
      <p class="text-xs mt-2 font-mono text-slate-700">
        {$currentVersion.content.slice(0, 60)}...
      </p>
    {/if}
  </div>
</div>
