import { invoke } from '@tauri-apps/api/core';

export type Item = { name: string; size: number; sha256: string };
export type CaptureProvenance = { lfs?: {objects:number;bytes:number}|null; url: string; advertisedRefCount: number; headAdvertised: boolean; inventory: { headSubmodules: number; headLfsPointers: number; smallBlobsScanned: number; smallBlobsTotal: number; lfsScanComplete: boolean } };
export type Job = { id: number; label: string; state: 'running'|'completed'|'failed'|'cancelled'; phase: string; startedAt: number; finishedAt: number|null; error: string|null; cancellable: boolean; result?: CaptureProvenance|null };
export type Status = {
  version: string; platform: string; vault: { path: string; itemCount: number; totalBytes: number }|null;
  workspace: { path: string; encryption: string }|null; busy: boolean; jobs: Job[]; remoteCapture: boolean; plan?: PlanSummary|null; newMigration?: NewMigration|null; migrationResult?: MigrationResult|null;
};
export type Action =
  | { kind: 'create_vault'|'unlock_vault'; path: string; passphrase: string }
  | { kind: 'check_workspace'; path: string; create: boolean }
  | { kind: 'capture_local'; name: string; source: string; includeLfs: boolean }
  | { kind: 'capture_remote'; name: string; url: string; username: string|null; token: string|null; caFile: string|null; includeLfs: boolean }
  | { kind: 'prepare_plan'; name: string; provider: 'github'|'gitlab'; instance: string|null; repository: string; token: string|null; username: string|null; caFile: string|null }
  | { kind:'prepare_new_migration'; name:string; provider:'github'|'gitlab'; instance:string|null; repository:string; token:string; username:string|null; caFile:string|null }
  | { kind:'execute_new_migration'; planId:string; confirmation:string; token:string; username:string|null; caFile:string|null }
  | { kind: 'restore_git'; name: string; destination: string }
  | { kind: 'create_backup'; destination: string }
  | { kind: 'verify_backup'; path: string; passphrase: string }
  | { kind: 'restore_backup'; path: string; destination: string; passphrase: string };

export const desktop = '__TAURI_INTERNALS__' in window;
export const emptyStatus: Status = { version: '0.1.0-alpha.7', platform: 'linux', vault: null, workspace: null, busy: false, jobs: [], remoteCapture: true };
export async function getStatus(): Promise<Status> { return desktop ? invoke<Status>('get_status') : emptyStatus; }
function requireDesktop() { if (!desktop) throw new Error('Open the desktop app to access local vaults and encrypted working storage.'); }
export async function startJob(action: Action): Promise<number> { requireDesktop(); return invoke<number>('start_job', { action }); }
export async function lockVault() { requireDesktop(); return invoke('lock_vault'); }
export async function cancelJob() { requireDesktop(); return invoke('cancel_job'); }
export async function listItems(query: string, offset: number, limit = 20): Promise<{ items: Item[]; total: number }> { requireDesktop(); return invoke('get_items', { query, offset, limit }); }
export async function choosePath(purpose: 'directory'|'backup_open'|'backup_save'|'key_open'): Promise<string|null> { requireDesktop(); return invoke<string|null>('choose_path', { purpose }); }

export type PlanRow = { name: string; sourceOid: string|null; destinationOid: string|null; change: string };
export type ApiIssue = { code: string; status: number|null; retryAfterSeconds: number|null };
export type PlanSummary = { schema: number; observedAt: number; provider: string; deployment: string; instance: string; requestedRepository: string; sourceName: string; sourceSha256: string; sourceVerification: string; actor: {id:number; login:string}|null; repository: {id:number; path:string; cloneUrl:string; archived:boolean; disabled:boolean; defaultBranch:string|null; pushReported:boolean|null; accessLevel:number|null}|null; policies: {endpoint:string; complete:boolean; count:number; issue:ApiIssue|null}[]; blockers: string[]; warnings: string[]; apiIssues: ApiIssue[]; destinationFingerprint: string|null; destinationHead: string|null; sourceExcludedRefs: number; destinationExcludedRefs: number; rows: number; counts: Record<string,number>; executionSupported: boolean; writeAuthorization: string };
export async function listPlanRows(query: string, offset: number, limit=20): Promise<{rows:PlanRow[]; total:number}> { requireDesktop(); return invoke('get_plan_rows',{query,offset,limit}); }

export type NewMigration = { id:string; expiresAt:number; provider:string; instance:string; deployment:string; repository:string; actor:{id:number;login:string}; sourceName:string; sourceSha256:string; defaultBranch:string; refCount:number; refs:Record<string,string>; excludedRefs:number; lfsObjects?:number; lfsBytes?:number; visibility:string; automation:string; scopeProof:string };
export type MigrationResult = {id:string;repository:string;repositoryId:number|null;state:string;lastPhase:string;receiptPrefix:string;failureCode?:string|null};
