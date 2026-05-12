<script lang="ts">
  import { onMount } from 'svelte';
  import { getSettings, saveSettings, startGoogleOAuth, waitGoogleOAuthCallback, connectRemoteSpace, syncSpace, disconnectSpace, getPages, getSpaces, getSpaceConfig, setSpaceConfig, getSpaceToken, exchangeGoogleToken, exchangeAdminToken, updateSpaceToken, listInvites, addInvite, removeInvite, refreshGoogleToken, type InviteEntry } from '$lib/api';
  import { activityStart, activityDone, activityError, userName, userEmail, spaces, currentSpace, pages } from '$lib/stores';
  import { User, Database, Chrome, HardDrive, Cloud, Unplug, RefreshCw, Sparkles } from 'lucide-svelte';

  let { onclose }: { onclose: () => void } = $props();

  let displayName      = $state('');
  let email            = $state('');
  let sqldUrl          = $state('');
  let sqldToken        = $state('');
  let openrouterApiKey = $state('');
  let googleClientId   = $state('');
  let googleClientSecret = $state('');
  let googleAccessToken  = $state('');
  let googleRefreshToken = $state('');
  let saving           = $state(false);
  let loaded           = $state(false);
  let connectingGoogle = $state(false);
  let googleConnected  = $state(false);

  // Remote server connect form
  let remoteUrl        = $state('');
  let remoteNamespace  = $state('');
  let remoteToken      = $state('');
  let remotePermission = $state('read');
  let connectingRemote = $state(false);
  let syncingSpaceId   = $state<string | null>(null);
  let disconnectingSpaceId = $state<string | null>(null);
  let connectMode      = $state<'manual' | 'invite'>('manual');
  let invitePaste      = $state('');
  let inviteError      = $state('');
  let copiedSpaceId    = $state<string | null>(null);
  let invitePermission = $state<Record<string, string>>({});

  // Invite management (owner spaces)
  let invitesBySpace   = $state<Record<string, InviteEntry[]>>({});
  let newInviteEmail   = $state<Record<string, string>>({});
  let inviteLoading    = $state<Record<string, boolean>>({});
  let expandedInvites  = $state<Record<string, boolean>>({});

  onMount(async () => {
    const s = await getSettings();
    displayName        = s.user_name           ?? '';
    email              = s.user_email          ?? '';
    sqldUrl            = s.sqld_url            ?? '';
    sqldToken          = s.sqld_token          ?? '';
    googleClientId     = s.google_client_id    ?? '';
    googleClientSecret = s.google_client_secret ?? '';
    googleAccessToken  = s.google_access_token  ?? '';
    googleRefreshToken = s.google_refresh_token ?? '';
    googleConnected    = !!googleRefreshToken;
    openrouterApiKey   = s.openrouter_api_key ?? '';
    loaded = true;
    // Auto-load synthesis config for all remote spaces — no hidden accordions
    for (const space of $spaces.filter(sp => sp.source === 'remote')) {
      loadSynthConfig(space.id);
    }
  });

  async function handleSave() {
    saving = true;
    const id = activityStart('Save settings');
    try {
      const current = await getSettings();
      await saveSettings(
        sqldUrl.trim() || null,
        sqldToken.trim() || null,
        googleClientId.trim() || current.google_client_id || null,
        googleClientSecret.trim() || current.google_client_secret || null,
        googleAccessToken || current.google_access_token || null,
        googleRefreshToken || current.google_refresh_token || null,
        displayName.trim() || null,
        email.trim() || null,
        openrouterApiKey.trim() || null,
      );
      // Update stores so sidebar profile updates immediately
      $userName = displayName.trim() || $userName;
      $userEmail = email.trim() || $userEmail;
      activityDone(id, sqldUrl.trim() ? 'Restart app to apply sqld URL' : 'Saved');
      onclose();
    } catch (e) {
      activityError(id, String(e));
    } finally {
      saving = false;
    }
  }

  async function handleConnectGoogle() {
    connectingGoogle = true;
    const id = activityStart('Connect Google account');
    try {
      await startGoogleOAuth('');
      const tokens = await waitGoogleOAuthCallback('', '');
      googleAccessToken  = tokens.access_token;
      googleRefreshToken = tokens.refresh_token;
      googleConnected = true;
      activityDone(id, 'Google account connected');
    } catch (e) {
      activityError(id, String(e));
    } finally {
      connectingGoogle = false;
    }
  }

  function handleDisconnectGoogle() {
    googleAccessToken  = '';
    googleRefreshToken = '';
    googleConnected    = false;
  }

  async function handleConnectRemote() {
    if (!remoteUrl.trim()) return;
    const nameToUse = remoteNamespace.trim() || new URL(remoteUrl.trim()).host;
    connectingRemote = true;
    const id = activityStart(`Connect ${nameToUse}`);
    try {
      const space = await connectRemoteSpace(
        remoteUrl.trim(),
        nameToUse,
        remoteToken.trim(),
        remotePermission,
        undefined, // no parent — top-level
      );
      $spaces = [...$spaces, space];
      remoteUrl = '';
      remoteNamespace = '';
      remoteToken = '';
      remotePermission = 'read';
      activityDone(id, `Connected ${space.name}`);
    } catch (e) {
      activityError(id, String(e));
    } finally {
      connectingRemote = false;
    }
  }

  async function handleSyncSpace(spaceId: string) {
    syncingSpaceId = spaceId;
    const id = activityStart('Sync space');
    try {
      // Remote spaces use new_remote (always live) — just reload pages
      if ($currentSpace?.id === spaceId) {
        $pages = await getPages(spaceId);
      }
      activityDone(id, 'Refreshed');
    } catch (e) {
      activityError(id, String(e));
    } finally {
      syncingSpaceId = null;
    }
  }

  let synthConfigs = $state<Record<string, { apiKey: string; model: string; role: string; saving: boolean; loaded: boolean }>>({});

  async function loadSynthConfig(spaceId: string) {
    if (synthConfigs[spaceId]?.loaded) return;
    try {
      const cfg = await getSpaceConfig(spaceId);
      synthConfigs = { ...synthConfigs, [spaceId]: { apiKey: cfg?.api_key ?? '', model: cfg?.model ?? 'minimax/minimax-m2.5', role: cfg?.synthesizer_role ?? 'owner', saving: false, loaded: true } };
    } catch {
      synthConfigs = { ...synthConfigs, [spaceId]: { apiKey: '', model: 'minimax/minimax-m2.5', role: 'owner', saving: false, loaded: true } };
    }
  }

  async function saveSynthConfig(spaceId: string) {
    const cfg = synthConfigs[spaceId];
    if (!cfg) return;
    synthConfigs = { ...synthConfigs, [spaceId]: { ...cfg, saving: true } };
    try {
      await setSpaceConfig(spaceId, cfg.apiKey, cfg.model, cfg.role);
      synthConfigs = { ...synthConfigs, [spaceId]: { ...cfg, saving: false } };
    } catch (e) { console.error(e); synthConfigs = { ...synthConfigs, [spaceId]: { ...cfg, saving: false } }; }
  }

  async function handleDisconnectSpace(spaceId: string) {
    disconnectingSpaceId = spaceId;
    const id = activityStart('Disconnect space');
    try {
      await disconnectSpace(spaceId);
      $spaces = $spaces.filter(s => s.id !== spaceId);
      activityDone(id, 'Disconnected');
    } catch (e) {
      activityError(id, String(e));
    } finally {
      disconnectingSpaceId = null;
    }
  }

  async function copyInvite(space: import('$lib/api').Space): Promise<void> {
    try {
      const token = await getSpaceToken(space.id);
      const perm = invitePermission[space.id] ?? 'write';
      const payload = {
        v: 1,
        serverUrl: space.server_url ?? '',
        name: space.name,
        token,
        permissionLevel: perm,
      };
      const link = btoa(JSON.stringify(payload));
      await navigator.clipboard.writeText(link);
      copiedSpaceId = space.id;
      setTimeout(() => { copiedSpaceId = null; }, 2000);
    } catch (e) {
      console.error('Failed to copy invite:', e);
    }
  }

  async function joinViaInvite(): Promise<void> {
    inviteError = '';
    try {
      const raw = invitePaste.trim().replace(/-/g, '+').replace(/_/g, '/');
      const padded = raw + '=='.slice(0, (4 - raw.length % 4) % 4);
      const decoded = JSON.parse(atob(padded));
      if (!decoded.serverUrl || !decoded.name) throw new Error('Invalid invite');

      let token = decoded.token ?? '';

      // Google auth flow: exchange current Google access token for a sqld JWT
      if (decoded.auth === 'google') {
        let gotToken = false;
        // Try Google access token first (with auto-refresh)
        if (googleAccessToken || googleRefreshToken) {
          let accessTok = googleAccessToken;
          if (!accessTok && googleRefreshToken && googleClientId && googleClientSecret) {
            accessTok = await refreshGoogleToken(googleClientId, googleClientSecret, googleRefreshToken);
            googleAccessToken = accessTok;
          }
          if (accessTok) {
            try {
              token = await exchangeGoogleToken(decoded.serverUrl, accessTok);
              gotToken = true;
            } catch (e) {
              // Expired — try refresh once
              if (googleRefreshToken && googleClientId && googleClientSecret) {
                try {
                  accessTok = await refreshGoogleToken(googleClientId, googleClientSecret, googleRefreshToken);
                  googleAccessToken = accessTok;
                  token = await exchangeGoogleToken(decoded.serverUrl, accessTok);
                  gotToken = true;
                } catch {}
              }
            }
          }
        }
        // Fall back to admin token if Google failed and invite includes one
        if (!gotToken && decoded.admin_token) {
          token = await exchangeAdminToken(decoded.serverUrl, decoded.admin_token);
          gotToken = true;
        }
        if (!gotToken) throw new Error('Sign in with Google first (Settings → Google)');
      }

      await connectRemoteSpace(
        decoded.serverUrl,
        decoded.name,
        token,
        decoded.permissionLevel ?? 'read',
        undefined,
        decoded.admin_token ?? undefined,
      );
      $spaces = await getSpaces();
      invitePaste = '';
      connectMode = 'manual';
    } catch (e) {
      console.error('joinViaInvite error:', e);
      inviteError = String(e);
    }
  }

  async function loadInvites(spaceId: string): Promise<void> {
    inviteLoading = { ...inviteLoading, [spaceId]: true };
    try {
      invitesBySpace = { ...invitesBySpace, [spaceId]: await listInvites(spaceId) };
    } catch (e) {
      console.error('listInvites error:', e);
    } finally {
      inviteLoading = { ...inviteLoading, [spaceId]: false };
    }
  }

  async function handleAddInvite(spaceId: string): Promise<void> {
    const email = (newInviteEmail[spaceId] ?? '').trim();
    if (!email) return;
    inviteLoading = { ...inviteLoading, [spaceId]: true };
    try {
      await addInvite(spaceId, email, $userName ?? undefined);
      newInviteEmail = { ...newInviteEmail, [spaceId]: '' };
      await loadInvites(spaceId);
    } catch (e) {
      console.error('addInvite error:', e);
    } finally {
      inviteLoading = { ...inviteLoading, [spaceId]: false };
    }
  }

  async function handleRemoveInvite(spaceId: string, email: string): Promise<void> {
    inviteLoading = { ...inviteLoading, [spaceId]: true };
    try {
      await removeInvite(spaceId, email);
      await loadInvites(spaceId);
    } catch (e) {
      console.error('removeInvite error:', e);
    } finally {
      inviteLoading = { ...inviteLoading, [spaceId]: false };
    }
  }

  function toggleInvites(spaceId: string): void {
    const open = !expandedInvites[spaceId];
    expandedInvites = { ...expandedInvites, [spaceId]: open };
    if (open && !invitesBySpace[spaceId]) loadInvites(spaceId);
  }

  function generateOwnerInvite(space: import('$lib/api').Space): void {
    const payload = JSON.stringify({
      v: 2,
      serverUrl: space.server_url ?? '',
      name: space.name,
      auth: 'google',
      permissionLevel: 'write',
    });
    const link = btoa(payload);
    navigator.clipboard.writeText(link);
    copiedSpaceId = space.id;
    setTimeout(() => { copiedSpaceId = null; }, 2000);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div
  class="fixed inset-0 z-50 flex items-center justify-center"
  style="background: rgba(0,0,0,0.45);"
  role="button"
  tabindex="-1"
  onclick={onclose}
  onkeydown={() => {}}
>
  <div
    class="rounded-2xl shadow-ambient w-full max-w-md mx-4 overflow-hidden max-h-[90vh] flex flex-col"
    style="background: var(--color-surface); border: 1px solid var(--color-border);"
    role="dialog"
    onclick={(e) => e.stopPropagation()}
    onkeydown={() => {}}
  >
    <!-- Header -->
    <div class="flex items-center justify-between px-6 py-4" style="border-bottom: 1px solid var(--color-border);">
      <h2 class="text-sm font-semibold" style="color: var(--color-on-surface);">Settings</h2>
      <button
        onclick={onclose}
        class="w-6 h-6 rounded-full flex items-center justify-center text-xs transition-colors"
        style="color: var(--color-on-muted); background: var(--color-surface-lo);"
      >✕</button>
    </div>

    <div class="overflow-y-auto flex-1 px-6 py-5">
      {#if !loaded}
        <p class="text-sm" style="color: var(--color-on-muted);">Loading…</p>
      {:else}
        <div class="space-y-6">

          <!-- Profile section -->
          <div>
            <div class="flex items-center gap-2 mb-3">
              <User size={13} style="color: var(--color-on-muted);" />
              <h3 class="text-xs font-semibold uppercase tracking-widest" style="color: var(--color-on-muted);">Profile</h3>
            </div>
            <div class="space-y-3">
              <label class="block">
                <span class="text-xs block mb-1" style="color: var(--color-on-muted);">Display Name</span>
                <input
                  type="text"
                  bind:value={displayName}
                  placeholder="Your name"
                  class="w-full text-sm px-3 py-2 rounded-lg outline-none transition-colors"
                  style="background: var(--color-surface-lo); color: var(--color-on-surface); border: 1px solid var(--color-border);"
                />
              </label>
              <label class="block">
                <span class="text-xs block mb-1" style="color: var(--color-on-muted);">Email <span style="opacity: 0.5;">(optional)</span></span>
                <input
                  type="email"
                  bind:value={email}
                  placeholder="you@example.com"
                  class="w-full text-sm px-3 py-2 rounded-lg outline-none transition-colors"
                  style="background: var(--color-surface-lo); color: var(--color-on-surface); border: 1px solid var(--color-border);"
                />
              </label>
            </div>
          </div>

          <div style="border-top: 1px solid var(--color-border);"></div>

          <!-- Global API Key -->
          <div>
            <div class="flex items-center gap-2 mb-3">
              <Sparkles size={13} style="color: var(--color-on-muted);" />
              <h3 class="text-xs font-semibold uppercase tracking-widest" style="color: var(--color-on-muted);">AI / OpenRouter</h3>
            </div>
            <p class="text-xs mb-3" style="color: var(--color-on-muted); opacity: 0.6;">
              Global API key used for synthesis on all spaces. Per-space keys (in space settings below) take priority.
            </p>
            <label class="block">
              <span class="text-xs block mb-1" style="color: var(--color-on-muted);">OpenRouter API Key</span>
              <input
                type="password"
                bind:value={openrouterApiKey}
                placeholder="sk-or-..."
                class="w-full text-sm px-3 py-2 rounded-lg outline-none transition-colors"
                style="background: var(--color-surface-lo); color: var(--color-on-surface); border: 1px solid var(--color-border);"
              />
            </label>
          </div>

          <div style="border-top: 1px solid var(--color-border);"></div>

          <!-- sqld section -->
          <div>
            <div class="flex items-center gap-2 mb-3">
              <Database size={13} style="color: var(--color-on-muted);" />
              <h3 class="text-xs font-semibold uppercase tracking-widest" style="color: var(--color-on-muted);">sqld Replication</h3>
            </div>
            <p class="text-xs mb-3" style="color: var(--color-on-muted); opacity: 0.6;">
              Sync documents to your remote sqld server. Leave blank for local-only mode. Restart to apply.
            </p>
            <div class="space-y-3">
              <label class="block">
                <span class="text-xs block mb-1" style="color: var(--color-on-muted);">Server URL</span>
                <input
                  type="text"
                  bind:value={sqldUrl}
                  placeholder="libsql://my-server:8093"
                  class="w-full text-sm px-3 py-2 rounded-lg outline-none transition-colors"
                  style="background: var(--color-surface-lo); color: var(--color-on-surface); border: 1px solid var(--color-border);"
                />
              </label>
              <label class="block">
                <span class="text-xs block mb-1" style="color: var(--color-on-muted);">Auth Token <span style="opacity: 0.5;">(optional)</span></span>
                <input
                  type="password"
                  bind:value={sqldToken}
                  placeholder="Leave empty for unauthenticated"
                  class="w-full text-sm px-3 py-2 rounded-lg outline-none transition-colors"
                  style="background: var(--color-surface-lo); color: var(--color-on-surface); border: 1px solid var(--color-border);"
                />
              </label>
            </div>
          </div>

          <div style="border-top: 1px solid var(--color-border);"></div>

          <!-- Remote Servers section -->
          <div>
            <div class="flex items-center gap-2 mb-3">
              <Cloud size={13} style="color: var(--color-on-muted);" />
              <h3 class="text-xs font-semibold uppercase tracking-widest" style="color: var(--color-on-muted);">Remote Servers</h3>
            </div>

            <!-- Connected remote spaces list -->
            {#if $spaces.filter(s => s.source === 'remote').length > 0}
              <div class="space-y-2 mb-4">
                {#each $spaces.filter(s => s.source === 'remote') as space (space.id)}
                  <div class="p-3 rounded-xl" style="background: var(--color-surface-lo); border: 1px solid var(--color-border);">
                    <!-- Space header row -->
                    <div class="flex items-center justify-between">
                      <div class="min-w-0 flex-1">
                        <p class="text-xs font-medium truncate" style="color: var(--color-on-surface);">{space.namespace ?? space.name}</p>
                        <p class="text-xs truncate" style="color: var(--color-on-muted); opacity: 0.6;">{space.server_url} · {space.permission_level}</p>
                      </div>
                      <div class="flex items-center gap-2 ml-3 shrink-0">
                        {#if space.permission_level === 'owner'}
                          <button
                            onclick={() => generateOwnerInvite(space)}
                            title="Copy invite link (Google auth)"
                            class="text-xs px-2 py-1 rounded transition-colors bg-slate-700 hover:bg-slate-600 text-slate-300"
                          >
                            {copiedSpaceId === space.id ? 'Copied!' : 'Copy Invite'}
                          </button>
                        {/if}
                        <button
                          onclick={() => handleSyncSpace(space.id)}
                          disabled={syncingSpaceId === space.id}
                          title="Sync now"
                          class="w-6 h-6 flex items-center justify-center rounded transition-colors disabled:opacity-40"
                          style="color: var(--color-on-muted);"
                        >
                          <RefreshCw size={12} class={syncingSpaceId === space.id ? 'animate-spin' : ''} />
                        </button>
                        <button
                          onclick={() => handleDisconnectSpace(space.id)}
                          disabled={disconnectingSpaceId === space.id}
                          title="Disconnect"
                          class="w-6 h-6 flex items-center justify-center rounded transition-colors disabled:opacity-40"
                          style="color: var(--color-on-muted);"
                        >
                          <Unplug size={12} />
                        </button>
                      </div>
                    </div>

                    <!-- Invite management (owner only) -->
                    {#if space.permission_level === 'owner'}
                      <details class="mt-2" ontoggle={(e) => { if ((e.target as HTMLDetailsElement).open) loadInvites(space.id); }}>
                        <summary class="cursor-pointer text-xs py-1 select-none" style="color: var(--color-on-muted);">👥 Manage access</summary>
                        <div class="mt-2 pl-2 border-l-2 border-slate-700 space-y-2">
                          {#if inviteLoading[space.id]}
                            <p class="text-xs" style="color: var(--color-on-muted); opacity: 0.5;">Loading…</p>
                          {:else}
                            {#each (invitesBySpace[space.id] ?? []) as entry (entry.email)}
                              <div class="flex items-center justify-between text-xs" style="color: var(--color-on-surface);">
                                <span>{entry.email}</span>
                                <button
                                  onclick={() => handleRemoveInvite(space.id, entry.email)}
                                  class="text-red-400 hover:text-red-300 ml-2"
                                >Remove</button>
                              </div>
                            {/each}
                            {#if (invitesBySpace[space.id] ?? []).length === 0}
                              <p class="text-xs" style="color: var(--color-on-muted); opacity: 0.5;">No one invited yet.</p>
                            {/if}
                          {/if}
                          <!-- Add email -->
                          <div class="flex gap-2 pt-1">
                            <input
                              type="email"
                              placeholder="user@example.com"
                              bind:value={newInviteEmail[space.id]}
                              onkeydown={(e) => e.key === 'Enter' && handleAddInvite(space.id)}
                              class="flex-1 text-xs rounded px-2 py-1 outline-none"
                              style="background: var(--color-surface); border: 1px solid var(--color-border); color: var(--color-on-surface);"
                            />
                            <button
                              onclick={() => handleAddInvite(space.id)}
                              disabled={!newInviteEmail[space.id]?.trim()}
                              class="text-xs px-2 py-1 rounded bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-40"
                            >Invite</button>
                          </div>
                        </div>
                      </details>
                    {/if}

                    <!-- Synthesis config — always visible, auto-loads -->
                    <div class="mt-3 pt-3 border-t border-slate-700">
                      <p class="text-xs font-semibold mb-2" style="color: var(--color-on-muted);">⚙ Synthesis (OpenRouter)</p>
                      {#if !synthConfigs[space.id]?.loaded}
                        <p class="text-xs" style="color: var(--color-on-muted); opacity: 0.5;">Loading…</p>
                      {:else}
                        <div class="space-y-2">
                          <div>
                            <label class="text-xs block mb-1" style="color: var(--color-on-muted);">OpenRouter API Key</label>
                            <input type="text"
                              class="w-full text-xs rounded px-2 py-1.5 outline-none font-mono"
                              style="background: var(--color-surface); border: 1px solid var(--color-border); color: var(--color-on-surface);"
                              placeholder="sk-or-..."
                              bind:value={synthConfigs[space.id].apiKey} />
                          </div>
                          <div>
                            <label class="text-xs block mb-1" style="color: var(--color-on-muted);">Model</label>
                            <input type="text"
                              class="w-full text-xs rounded px-2 py-1.5 outline-none"
                              style="background: var(--color-surface); border: 1px solid var(--color-border); color: var(--color-on-surface);"
                              bind:value={synthConfigs[space.id].model} />
                          </div>
                          <div>
                            <label class="text-xs block mb-1" style="color: var(--color-on-muted);">Who can synthesize</label>
                            <select class="text-xs rounded px-2 py-1.5 outline-none"
                              style="background: var(--color-surface); border: 1px solid var(--color-border); color: var(--color-on-surface);"
                              bind:value={synthConfigs[space.id].role}>
                              <option value="owner">Owner only</option>
                              <option value="writer">All writers</option>
                            </select>
                          </div>
                          <button onclick={() => saveSynthConfig(space.id)}
                            disabled={synthConfigs[space.id].saving}
                            class="px-3 py-1 rounded text-xs bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-50">
                            {synthConfigs[space.id].saving ? 'Saving…' : 'Save API Key'}
                          </button>
                          {#if synthConfigs[space.id].apiKey}
                            <p class="text-xs text-green-500">✓ Key configured</p>
                          {:else}
                            <p class="text-xs text-amber-400">⚠ No key — synthesis will fail</p>
                          {/if}
                        </div>
                      {/if}
                    </div>
                  </div>
                {/each}
              </div>
            {/if}

            <!-- Connect new namespace — mode toggle -->
            <div class="flex items-center gap-3 mb-3">
              <button
                onclick={() => connectMode = 'manual'}
                class="text-xs font-medium transition-colors pb-0.5"
                style={connectMode === 'manual'
                  ? 'color: var(--color-on-surface); border-bottom: 1px solid var(--color-on-surface);'
                  : 'color: var(--color-on-muted); border-bottom: 1px solid transparent;'}
              >Manual</button>
              <button
                onclick={() => connectMode = 'invite'}
                class="text-xs font-medium transition-colors pb-0.5"
                style={connectMode === 'invite'
                  ? 'color: var(--color-on-surface); border-bottom: 1px solid var(--color-on-surface);'
                  : 'color: var(--color-on-muted); border-bottom: 1px solid transparent;'}
              >Invite Link</button>
            </div>

            {#if connectMode === 'manual'}
              <div class="space-y-2">
                <label class="block">
                  <span class="text-xs block mb-1" style="color: var(--color-on-muted);">Server URL</span>
                  <input
                    type="text"
                    bind:value={remoteUrl}
                    placeholder="http://127.0.0.1:8093  or  http://127.0.0.1:8095"
                    class="w-full text-sm px-3 py-2 rounded-lg outline-none transition-colors"
                    style="background: var(--color-surface-lo); color: var(--color-on-surface); border: 1px solid var(--color-border);"
                  />
                </label>
                <label class="block">
                  <span class="text-xs block mb-1" style="color: var(--color-on-muted);">Name <span style="opacity: 0.5;">(optional)</span></span>
                  <input
                    type="text"
                    bind:value={remoteNamespace}
                    placeholder="shared"
                    class="w-full text-sm px-3 py-2 rounded-lg outline-none transition-colors"
                    style="background: var(--color-surface-lo); color: var(--color-on-surface); border: 1px solid var(--color-border);"
                  />
                </label>
                <label class="block">
                  <span class="text-xs block mb-1" style="color: var(--color-on-muted);">Auth Token <span style="opacity: 0.5;">(optional)</span></span>
                  <input
                    type="password"
                    bind:value={remoteToken}
                    placeholder="Leave empty for unauthenticated"
                    class="w-full text-sm px-3 py-2 rounded-lg outline-none transition-colors"
                    style="background: var(--color-surface-lo); color: var(--color-on-surface); border: 1px solid var(--color-border);"
                  />
                </label>
                <label class="block">
                  <span class="text-xs block mb-1" style="color: var(--color-on-muted);">Permission</span>
                  <select
                    bind:value={remotePermission}
                    class="w-full text-sm px-3 py-2 rounded-lg outline-none transition-colors"
                    style="background: var(--color-surface-lo); color: var(--color-on-surface); border: 1px solid var(--color-border);"
                  >
                    <option value="read">read</option>
                    <option value="write">write</option>
                    <option value="owner">owner</option>
                  </select>
                </label>
                <button
                  onclick={handleConnectRemote}
                  disabled={connectingRemote || !remoteUrl.trim()}
                  class="w-full py-2 text-sm rounded-xl transition-colors disabled:opacity-40 flex items-center justify-center gap-2 font-medium"
                  style="background: var(--color-surface-lo); color: var(--color-on-surface); border: 1px solid var(--color-border);"
                >
                  {#if connectingRemote}
                    <span class="animate-spin w-3 h-3 border border-current border-t-transparent rounded-full inline-block"></span>
                    Connecting…
                  {:else}
                    Connect
                  {/if}
                </button>
              </div>
            {:else}
              <div class="flex flex-col gap-2">
                <label for="invite-paste" class="text-xs" style="color: var(--color-on-muted);">Paste invite link</label>
                <textarea
                  id="invite-paste"
                  bind:value={invitePaste}
                  rows="3"
                  placeholder="Paste the invite code here..."
                  class="bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-slate-100 text-sm font-mono
                         focus:outline-none focus:border-indigo-500 placeholder-slate-600 resize-none"
                ></textarea>
                {#if inviteError}
                  <p class="text-xs text-red-400">{inviteError}</p>
                {/if}
                <button
                  onclick={joinViaInvite}
                  disabled={!invitePaste.trim()}
                  class="w-full py-2 text-sm rounded-xl transition-colors disabled:opacity-40 flex items-center justify-center gap-2 font-medium"
                  style="background: var(--color-surface-lo); color: var(--color-on-surface); border: 1px solid var(--color-border);"
                >
                  Join Space
                </button>
              </div>
            {/if}
          </div>

          <div style="border-top: 1px solid var(--color-border);"></div>

          <!-- Google OAuth section -->
          <div>
            <div class="flex items-center gap-2 mb-3">
              <Chrome size={13} style="color: var(--color-on-muted);" />
              <h3 class="text-xs font-semibold uppercase tracking-widest" style="color: var(--color-on-muted);">Google Docs</h3>
            </div>

            {#if googleConnected}
              <div class="flex items-center justify-between p-3 rounded-xl" style="background: #f0fdf4; border: 1px solid #bbf7d0;">
                <div>
                  <p class="text-xs font-medium" style="color: #15803d;">Google account connected</p>
                  <p class="text-xs mt-0.5" style="color: #166534; opacity: 0.7;">Private docs can now be imported</p>
                </div>
                <button
                  onclick={handleDisconnectGoogle}
                  class="text-xs transition-colors"
                  style="color: var(--color-on-muted);"
                >Disconnect</button>
              </div>
            {:else}
              <p class="text-xs mb-3" style="color: var(--color-on-muted); opacity: 0.6;">
                Connect your Google account to import private Google Docs.
              </p>
              <button
                onclick={handleConnectGoogle}
                disabled={connectingGoogle}
                class="w-full py-2 text-sm rounded-xl transition-colors disabled:opacity-40 flex items-center justify-center gap-2 font-medium"
                style="background: var(--color-surface-lo); color: var(--color-on-surface); border: 1px solid var(--color-border);"
              >
                {#if connectingGoogle}
                  <span class="animate-spin w-3 h-3 border border-current border-t-transparent rounded-full inline-block"></span>
                  Waiting for browser…
                {:else}
                  Connect Google Account
                {/if}
              </button>
            {/if}
          </div>

          <div style="border-top: 1px solid var(--color-border);"></div>

          <!-- Local DB info -->
          <div>
            <div class="flex items-center gap-2 mb-2">
              <HardDrive size={13} style="color: var(--color-on-muted);" />
              <h3 class="text-xs font-semibold uppercase tracking-widest" style="color: var(--color-on-muted);">Local Database</h3>
            </div>
            <p class="text-xs font-mono" style="color: var(--color-on-muted); opacity: 0.6;">~/.bamako/local.db</p>
          </div>

        </div>
      {/if}
    </div>

    <!-- Footer -->
    <div class="flex justify-end gap-2 px-6 py-4" style="border-top: 1px solid var(--color-border);">
      <button
        onclick={onclose}
        class="px-4 py-2 text-sm rounded-xl transition-colors"
        style="color: var(--color-on-muted);"
      >Cancel</button>
      <button
        onclick={handleSave}
        disabled={saving}
        class="px-4 py-2 text-sm rounded-xl font-medium transition-colors disabled:opacity-50"
        style="background: var(--color-primary); color: white;"
      >{saving ? 'Saving…' : 'Save'}</button>
    </div>
  </div>
</div>
