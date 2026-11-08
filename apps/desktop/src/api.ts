import { invoke } from '@tauri-apps/api/core';

export type Item = { name: string; size: number; sha256: string };
export type Job = { id: number; label: string; state: 'running'|'completed'|'failed'|'cancelled'; phase: string; startedAt: number; finishedAt: number|null; error: string|null; cancellable: boolean };
export type Status = {
  version: string; platform: string; vault: { path: string; itemCount: number; totalBytes: number }|null;
  workspace: { path: string; encryption: string }|null; busy: boolean; jobs: Job[]; remoteCapture: boolean;
};
export type Action =
  | { kind: 'create_vault'|'unlock_vault'; path: string; passphrase: string }
  | { kind: 'check_workspace'; path: string; create: boolean }
  | { kind: 'capture_local'; name: string; source: string }
  | { kind: 'restore_git'; name: string; destination: string }
  | { kind: 'create_backup'; destination: string }
  | { kind: 'verify_backup'; path: string; passphrase: string }
  | { kind: 'restore_backup'; path: string; destination: string; passphrase: string };

export const desktop = '__TAURI_INTERNALS__' in window;
export const emptyStatus: Status = { version: '0.1.0-alpha.3', platform: 'linux', vault: null, workspace: null, busy: false, jobs: [], remoteCapture: false };
export async function getStatus(): Promise<Status> { return desktop ? invoke<Status>('get_status') : emptyStatus; }
function requireDesktop() { if (!desktop) throw new Error('Open the desktop app to access local vaults and encrypted working storage.'); }
export async function startJob(action: Action): Promise<number> { requireDesktop(); return invoke<number>('start_job', { action }); }
export async function lockVault() { requireDesktop(); return invoke('lock_vault'); }
export async function cancelJob() { requireDesktop(); return invoke('cancel_job'); }
export async function listItems(query: string, offset: number, limit = 20): Promise<{ items: Item[]; total: number }> { requireDesktop(); return invoke('get_items', { query, offset, limit }); }
export async function choosePath(purpose: 'directory'|'backup_open'|'backup_save'|'key_open'): Promise<string|null> { requireDesktop(); return invoke<string|null>('choose_path', { purpose }); }
