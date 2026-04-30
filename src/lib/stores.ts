import { writable } from 'svelte/store';
import type { Space, Page, PageVersion, SearchResult } from './api';

export const spaces = writable<Space[]>([]);
export const currentSpace = writable<Space | null>(null);
export const pages = writable<Page[]>([]);
export const currentPage = writable<Page | null>(null);
export const currentVersion = writable<PageVersion | null>(null);
export const versions = writable<PageVersion[]>([]);

// ── Theme ─────────────────────────────────────────────────────────────────────
function _saved(): 'dark' | 'light' {
  try { const v = localStorage.getItem('bamako-theme'); return v === 'light' ? 'light' : 'dark'; } catch { return 'dark'; }
}
function _persist(v: 'dark' | 'light') { try { localStorage.setItem('bamako-theme', v); } catch {} }

const _themeBase = writable<'dark' | 'light'>(_saved());
export const theme = {
  subscribe: _themeBase.subscribe,
  set(v: 'dark' | 'light') { _persist(v); _themeBase.set(v); },
  toggle() { _themeBase.update(v => { const n = v === 'dark' ? 'light' : 'dark'; _persist(n); return n; }); },
};

// ── User profile ──────────────────────────────────────────────────────────────
function _savedUserName(): string {
  try { return localStorage.getItem('bamako-user-name') ?? ''; } catch { return ''; }
}
function _persistUserName(v: string) { try { localStorage.setItem('bamako-user-name', v); } catch {} }

const _userNameBase = writable<string>(_savedUserName());
export const userName = {
  subscribe: _userNameBase.subscribe,
  set(v: string) { _persistUserName(v); _userNameBase.set(v); },
  update(fn: (v: string) => string) {
    _userNameBase.update(v => { const n = fn(v); _persistUserName(n); return n; });
  },
};

export const userEmail = writable<string>('');

// ── Read / Edit mode ──────────────────────────────────────────────────────────
export const readMode = writable<boolean>(false);

// ── Cmd+K search focus signal ─────────────────────────────────────────────────
export const searchFocusTick = writable<number>(0);

// ── Activity log ──────────────────────────────────────────────────────────────

export type ActivityStatus = 'running' | 'done' | 'error';
export type ActivityEvent = {
  id: string;
  label: string;
  status: ActivityStatus;
  detail?: string;
  ts: number;
};

export const activityLog = writable<ActivityEvent[]>([]);

// ── Synthesis signal ───────────────────────────────────────────────────────────
export const lastSynthesisAt = writable<number>(0);

// ── Search state (shared between Sidebar input and main panel display) ─────────
export const searchQuery = writable<string>('');
export const searchResults = writable<SearchResult[]>([]);

let _seq = 0;
export function activityStart(label: string): string {
  const id = `act-${++_seq}`;
  activityLog.update(log => [{ id, label, status: 'running', ts: Date.now() }, ...log].slice(0, 50));
  return id;
}
export function activityDone(id: string, detail?: string) {
  activityLog.update(log => log.map(e => e.id === id ? { ...e, status: 'done', detail } : e));
}
export function activityError(id: string, detail: string) {
  activityLog.update(log => log.map(e => e.id === id ? { ...e, status: 'error', detail } : e));
}
