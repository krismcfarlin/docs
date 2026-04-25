import { invoke } from '@tauri-apps/api/core';

export interface Space {
  id: string;
  name: string;
  description: string | null;
  parent_space_id: string | null;
  sort_order: number;
  created_at: string;
  /** "local" or "remote" */
  source: string;
  /** sqld namespace name (remote spaces only) */
  namespace: string | null;
  /** sqld server base URL — no token exposed to frontend */
  server_url: string | null;
  /** "owner" | "write" | "read" */
  permission_level: string;
}

export interface Page {
  id: string;
  title: string;
  space_id: string;
  creator_id: string;
  parent_page_id: string | null;
  sort_order: number;
  created_at: string;
  updated_at: string;
  /** "local" or a remote server URL */
  source: string;
  remote_id: string | null;
  /** "owner" | "write" | "read" */
  permission_level: string;
  last_synced_at: string | null;
}

export interface PageVersion {
  id: string;
  page_id: string;
  owner_id: string;
  based_on_version_id: string | null;
  title: string | null;
  content: string | null;
  text_content: string | null;
  is_published: boolean;
  is_frozen: boolean;
  version_num: number;
  created_at: string;
  updated_at: string;
}

export const initDb = () => invoke<void>('init_db');
export const syncDb = () => invoke<void>('sync_db');
export const getSpaces = () => invoke<Space[]>('get_spaces');
export const createSpace = (name: string, description?: string, parentSpaceId?: string) =>
  invoke<Space>('create_space', { name, description: description ?? null, parentSpaceId: parentSpaceId ?? null });
export const moveSpace = (spaceId: string, parentSpaceId: string | null) =>
  invoke<void>('move_space', { spaceId, parentSpaceId });
export const reorderSpaces = (ids: string[]) =>
  invoke<void>('reorder_spaces', { ids });
export const reorderPages = (ids: string[], spaceId: string) =>
  invoke<void>('reorder_pages', { ids, spaceId });
export const getPages = (spaceId: string) =>
  invoke<Page[]>('get_pages', { spaceId });
export const createPage = (title: string, spaceId: string, parentPageId?: string) =>
  invoke<Page>('create_page', { title, spaceId, parentPageId: parentPageId ?? null });
export const getPageVersion = (pageId: string, spaceId: string, versionId?: string) =>
  invoke<PageVersion | null>('get_page_version', { pageId, versionId: versionId ?? null, spaceId });
export type SaveResult =
  | { type: 'ok'; new_updated_at: string }
  | { type: 'conflict'; current_content: string; current_updated_at: string };

export const savePageVersion = (versionId: string, title: string, content: string, textContent: string, spaceId: string, baseUpdatedAt: string) =>
  invoke<SaveResult>('save_page_version', { versionId, title, content, textContent, spaceId, baseUpdatedAt });
export const publishVersion = (versionId: string, spaceId: string) =>
  invoke<void>('publish_version', { versionId, spaceId });
export const freezeVersion = (versionId: string, spaceId: string) =>
  invoke<void>('freeze_version', { versionId, spaceId });
export const forkVersion = (versionId: string, spaceId: string) =>
  invoke<PageVersion>('fork_version', { versionId, spaceId });
export const listPageVersions = (pageId: string, spaceId: string) =>
  invoke<PageVersion[]>('list_page_versions', { pageId, spaceId });

export const readFile = (path: string) =>
  invoke<string>('read_file', { path });
export const getPageImage = (objectId: string) =>
  invoke<string>('get_page_image', { objectId });
export const fetchGdoc = (url: string) =>
  invoke<string>('fetch_gdoc', { url });
export const importPage = (title: string, spaceId: string, content: string) =>
  invoke<string>('import_page', { title, spaceId, content });

export interface SearchResult {
  version_id: string;
  page_id: string;
  title: string;
  score: number;
  snippet: string;
}
export const vectorizePage = (versionId: string, spaceId: string) =>
  invoke<void>('vectorize_page', { versionId, spaceId });
export const searchSimilarPages = (spaceId: string, query: string, limit?: number) =>
  invoke<SearchResult[]>('search_similar_pages', { spaceId, query, limit: limit ?? 10 });

export const deletePage = (pageId: string, spaceId: string) =>
  invoke<void>('delete_page', { pageId, spaceId });
export const getTrashPages = (spaceId: string) =>
  invoke<Page[]>('get_trash_pages', { spaceId });
export const restorePage = (pageId: string, spaceId: string) =>
  invoke<void>('restore_page', { pageId, spaceId });
export const permanentDeletePage = (pageId: string, spaceId: string) =>
  invoke<void>('permanent_delete_page', { pageId, spaceId });
export const movePageToSpace = (pageId: string, fromSpaceId: string, toSpaceId: string) =>
  invoke<void>('move_page_to_space', { pageId, fromSpaceId, toSpaceId });
export const deleteSpace = (spaceId: string) =>
  invoke<void>('delete_space', { spaceId });
export const renamePage = (pageId: string, title: string, spaceId: string) =>
  invoke<void>('rename_page', { pageId, title, spaceId });
export const renameSpace = (spaceId: string, name: string) =>
  invoke<void>('rename_space', { spaceId, name });

export const recordPageAccess = (pageId: string, spaceId: string) =>
  invoke<void>('record_page_access', { pageId, spaceId });

export interface RecentPage {
  id: string;
  title: string;
  space_id: string;
  space_name: string;
  last_accessed_at: string;
  source: string;
  permission_level: string;
}
export const getRecentPages = (limit?: number) =>
  invoke<RecentPage[]>('get_recent_pages', { limit: limit ?? 8 });

export interface AppSettings {
  sqld_url:              string | null;
  sqld_token:            string | null;
  google_client_id:      string | null;
  google_client_secret:  string | null;
  google_access_token:   string | null;
  google_refresh_token:  string | null;
  user_name:             string | null;
  user_email:            string | null;
  openrouter_api_key:    string | null;
}
export const connectRemoteSpace = (serverUrl: string, namespace: string, token: string, permissionLevel: string, parentSpaceId?: string, adminToken?: string) =>
  invoke<Space>('connect_remote_space', { serverUrl, namespace, token, permissionLevel, parentSpaceId: parentSpaceId ?? null, adminToken: adminToken ?? null });

export const syncSpace = (spaceId: string) =>
  invoke<void>('sync_space', { spaceId });

export const disconnectSpace = (spaceId: string) =>
  invoke<void>('disconnect_space', { spaceId });

export const getSpaceToken = (spaceId: string) =>
  invoke<string>('get_space_token', { spaceId });

export const exchangeGoogleToken = (serverUrl: string, accessToken: string) =>
  invoke<string>('exchange_google_token', { serverUrl, accessToken });

export const exchangeAdminToken = (serverUrl: string, adminToken: string) =>
  invoke<string>('exchange_admin_token', { serverUrl, adminToken });

export const updateSpaceToken = (spaceId: string, token: string) =>
  invoke<void>('update_space_token', { spaceId, token });

export interface InviteEntry { email: string; added_at: number; added_by?: string; }
export const listInvites = (spaceId: string) =>
  invoke<InviteEntry[]>('list_invites', { spaceId });

export const addInvite = (spaceId: string, email: string, addedBy?: string) =>
  invoke<void>('add_invite', { spaceId, email, addedBy: addedBy ?? null });

export const removeInvite = (spaceId: string, email: string) =>
  invoke<void>('remove_invite', { spaceId, email });

export const getSettings = () => invoke<AppSettings>('get_settings');
export const saveSettings = (
  sqldUrl: string | null,
  sqldToken: string | null,
  googleClientId: string | null,
  googleClientSecret: string | null,
  googleAccessToken: string | null,
  googleRefreshToken: string | null,
  userName: string | null,
  userEmail: string | null,
  openrouterApiKey: string | null,
) => invoke<void>('save_settings', { sqldUrl, sqldToken, googleClientId, googleClientSecret, googleAccessToken, googleRefreshToken, userName, userEmail, openrouterApiKey });

export interface GoogleTokens { access_token: string; refresh_token: string; }
export const startGoogleOAuth = (clientId: string) =>
  invoke<void>('start_google_oauth', { clientId });
export const waitGoogleOAuthCallback = (clientId: string, clientSecret: string) =>
  invoke<GoogleTokens>('wait_google_oauth_callback', { clientId, clientSecret });
export const refreshGoogleToken = (clientId: string, clientSecret: string, refreshToken: string) =>
  invoke<string>('refresh_google_token', { clientId, clientSecret, refreshToken });

export interface Presence {
  id: string;
  user_name: string;
  page_id: string;
  page_title: string;
  space_id: string;
  status: 'viewing' | 'editing';
  last_seen_at: string;
}

export const upsertPresence = (spaceId: string, pageId: string, pageTitle: string, userName: string, status: 'viewing' | 'editing') =>
  invoke<void>('upsert_presence', { spaceId, pageId, pageTitle, userName, status });

export const clearPresence = (spaceId: string, pageId: string, userName: string) =>
  invoke<void>('clear_presence', { spaceId, pageId, userName });

export const getPagePresence = (spaceId: string, pageId: string) =>
  invoke<Presence[]>('get_page_presence', { spaceId, pageId });

export const getAllPresence = (spaceId: string) =>
  invoke<Presence[]>('get_all_presence', { spaceId });

// ── Synthesis ─────────────────────────────────────────────────────────────────

export interface SpaceConfig {
  api_key: string;
  model: string;
  synthesizer_role: string;
}

export interface PageSynthesis {
  page_id: string;
  summary: string;
  key_points: string[];
  topics: string[];
  synthesized_at: string;
}

export interface EntitySuggestion {
  id: string;
  name: string;
  entity_type: string;
  description: string;
  mention_count: number;
  status: string;
}

export interface SpaceOverview {
  overview: string;
  topics: string[];
  synthesized_at: string;
}

export interface PageLink {
  relationship: string;
  description: string;
  other_page_id: string;
  other_page_title: string;
}

export const getSpaceConfig = (spaceId: string) =>
  invoke<SpaceConfig | null>('get_space_config', { spaceId });

export const setSpaceConfig = (spaceId: string, apiKey: string, model: string, synthesizerRole: string) =>
  invoke<void>('set_space_config', { spaceId, apiKey, model, synthesizerRole });

export const synthesizePage = (spaceId: string, pageId: string) =>
  invoke<PageSynthesis>('synthesize_page', { spaceId, pageId });

export const getPageSynthesis = (spaceId: string, pageId: string) =>
  invoke<PageSynthesis | null>('get_page_synthesis', { spaceId, pageId });

export const getEntitySuggestions = (spaceId: string) =>
  invoke<EntitySuggestion[]>('get_entity_suggestions', { spaceId });

export const promoteEntity = (spaceId: string, entityId: string) =>
  invoke<string>('promote_entity', { spaceId, entityId });

export const dismissEntity = (spaceId: string, entityId: string) =>
  invoke<void>('dismiss_entity', { spaceId, entityId });

export const updateSpaceOverview = (spaceId: string) =>
  invoke<SpaceOverview>('update_space_overview', { spaceId });

export const getSpaceOverview = (spaceId: string) =>
  invoke<SpaceOverview | null>('get_space_overview', { spaceId });

export const getPageLinks = (spaceId: string, pageId: string) =>
  invoke<PageLink[]>('get_page_links', { spaceId, pageId });
