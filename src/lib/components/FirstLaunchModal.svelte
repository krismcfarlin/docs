<script lang="ts">
  import { userName } from '$lib/stores';
  import { getSettings, saveSettings } from '$lib/api';

  let nameInput = $state('');
  let saving = $state(false);

  async function save() {
    const trimmed = nameInput.trim();
    if (!trimmed || saving) return;
    saving = true;
    try {
      const current = await getSettings();
      await saveSettings(
        current.sqld_url ?? null,
        current.sqld_token ?? null,
        current.google_client_id ?? null,
        current.google_client_secret ?? null,
        current.google_access_token ?? null,
        current.google_refresh_token ?? null,
        trimmed,
        current.user_email ?? null,
      );
      $userName = trimmed;
    } catch {
      $userName = trimmed; // still update store even if persist fails
    } finally {
      saving = false;
    }
  }
</script>

{#if !$userName}
  <div class="fixed inset-0 z-[100] flex items-center justify-center bg-[#0f0f17]">
    <div class="w-full max-w-sm bg-[#1e1e2e] rounded-xl p-8 flex flex-col gap-6 shadow-2xl border border-slate-800">
      <div>
        <h1 class="text-xl font-semibold text-slate-100">Welcome to Bamako</h1>
        <p class="text-sm text-slate-400 mt-1">Enter your name so others can see who you are when collaborating.</p>
      </div>
      <div class="flex flex-col gap-2">
        <label class="text-xs text-slate-400 font-medium" for="name-input">Your name</label>
        <input
          id="name-input"
          type="text"
          bind:value={nameInput}
          onkeydown={(e) => e.key === 'Enter' && save()}
          placeholder="e.g. Alice"
          class="bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-slate-100 text-sm
                 focus:outline-none focus:border-indigo-500 placeholder-slate-600"
          autofocus
        />
      </div>
      <button
        onclick={save}
        disabled={!nameInput.trim() || saving}
        class="w-full py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 disabled:opacity-40
               disabled:cursor-not-allowed text-white text-sm font-medium transition-colors"
      >
        {saving ? 'Saving…' : 'Get Started'}
      </button>
    </div>
  </div>
{/if}
