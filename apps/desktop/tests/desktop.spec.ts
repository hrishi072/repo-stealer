import { expect, test, type Page } from '@playwright/test';
import type { Status } from '../src/api';

async function mockDesktop(page: Page, unlocked = false, workspace = false, plan = false, migration = false, platform = 'linux') {
  await page.addInitScript(({ unlocked, workspace, plan, migration, platform }) => {
    const state:Status = { version: '0.1.0-alpha.5', platform, vault: unlocked ? { path: '/synthetic/vault', itemCount: 1, totalBytes: 4096 } : null, workspace: workspace ? { path: '/synthetic/luks/work', encryption: 'LinuxLuks' } : null, busy: false, jobs: [] as unknown[], remoteCapture: true, plan: plan ? {schema:1,observedAt:10,provider:'gitlab',deployment:'self_hosted',instance:'https://git.example/platform/',requestedRepository:'group/sub/repo',sourceName:'history',sourceSha256:'a'.repeat(64),sourceVerification:'authenticated_manifest',actor:{id:1,login:'fixture'},repository:null,policies:[{endpoint:'protected_branches',complete:false,count:0,issue:{code:'access_denied',status:403,retryAfterSeconds:null}}],blockers:[],warnings:['policy_coverage_incomplete_never_assume_unprotected'],apiIssues:[],destinationFingerprint:'b'.repeat(64),destinationHead:null,sourceExcludedRefs:2,destinationExcludedRefs:1,rows:41,counts:{create_candidate:41},executionSupported:false,writeAuthorization:'unproven'} : null };
    state.newMigration=migration ? {id:'1'.repeat(32),expiresAt:Math.floor(Date.now()/1000)+900,provider:'github',instance:'https://github.com/',deployment:'cloud',repository:'fixture/new-private',actor:{id:10,login:'fixture'},sourceName:'history',sourceSha256:'a'.repeat(64),defaultBranch:'main',refCount:5,refs:{'refs/heads/main':'b'.repeat(40)},excludedRefs:2,visibility:'private',automation:'disabled',scopeProof:'classic_repo_workflow'} : null;
    const calls: { command: string; args: unknown }[] = [];
    Object.assign(window, { __desktopTestCalls: calls, __TAURI_INTERNALS__: { invoke: async (command: string, args: unknown) => {
      calls.push({ command, args });
      if (command === 'get_status') return state;
      if (command === 'get_workspace_setup') return {recommendedImage:'/synthetic/working-storage.rsworkspace',serviceAvailable:true,containerExists:false};
      if (command === 'get_migration_history') {
        const {offset,limit}=args as {offset:number;limit:number};
        return {total:21,items:Array.from({length:Math.min(limit,21-offset)},(_,i)=>({id:(offset+i+1).toString(16).padStart(32,'0'),repository:'fixture/recovered-private',createdAt:1,state:'interrupted_needs_attention'}))};
      }
      if (command === 'get_items') return { items: [{ name: '<img src=x onerror="window.untrustedRan=true">', size: 4096, sha256: 'a'.repeat(64) }], total: 1 };
      if (command === 'get_plan_rows') { const {offset,limit}=args as {offset:number;limit:number}; return {total:41,rows:Array.from({length:Math.min(limit,41-offset)},(_,n)=>({name:`refs/heads/branch-${offset+n}`,sourceOid:'c'.repeat(40),destinationOid:null,change:'create_candidate'}))}; }
      if (command === 'lock_vault') { state.vault = null; state.plan = null; state.newMigration=null;state.migrationResult=null;state.migrationHistory=null; return; }
      if (command === 'start_job') {
        const {action}=args as {action:{kind:string;planId:string}};
        if(action.kind==='inspect_migration')state.migrationHistory={
          plan:{id:action.planId,expiresAt:901,provider:'github',instance:'https://github.com/',deployment:'cloud',repository:'fixture/recovered-private',actor:{id:10,login:'fixture'},sourceName:'recovered-source',sourceSha256:'a'.repeat(64),defaultBranch:'main',refCount:1,refs:{'refs/heads/main':'b'.repeat(40)},excludedRefs:0,visibility:'private',automation:'disabled',scopeProof:'classic_repo_workflow'},
          state:'interrupted_needs_attention',lastPhase:'transferred',repositoryId:91,receipts:[{phase:'intent',time:2,repositoryId:null,failureCode:null},{phase:'transferred',time:2,repositoryId:null,failureCode:null}],reconciliations:[]};
        if(action.kind==='reconcile_migration' && state.migrationHistory)state.migrationHistory.reconciliations=[{schema:1,operation:action.planId,observedAt:10,state:'discrepancies',issues:['default_branch_differs'],differences:[{name:'refs/heads/main',expectedOid:'b'.repeat(40),observedOid:null}]}];
        return 1;
      }
      if (command === 'choose_path') return '/synthetic/chosen';
      throw new Error('Unexpected test command: ' + command);
    } } });
  }, { unlocked, workspace, plan, migration, platform });
}

test('browser preview is honest and capture stays gated', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByText('Browser preview', { exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Capture repository', exact: true }).first()).toBeDisabled();
  await page.getByRole('button', { name: 'Create new', exact: false }).click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await page.getByLabel('Vault location', { exact: true }).fill('/synthetic/new-vault');
  await page.getByLabel('Vault passphrase', { exact: true }).fill('synthetic testing passphrase');
  await page.getByLabel('Confirm passphrase', { exact: true }).fill('synthetic testing passphrase');
  await page.getByRole('button', { name: 'Create vault', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Open the desktop app');
  await expect(page.getByLabel('Vault passphrase', { exact: true })).toHaveValue('');
});

test('status polling does not steal keyboard focus while unlocking', async ({ page }) => {
  await mockDesktop(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Unlock vault', exact: true }).first().click();
  const path = page.getByLabel('Vault location', { exact: true });
  await path.fill('/synthetic/vault');
  const password = page.getByLabel('Vault passphrase', { exact: true });
  await password.fill('synthetic testing passphrase');
  await page.waitForTimeout(1600);
  await expect(password).toBeFocused();
  await page.getByRole('button', { name: 'Unlock vault', exact: true }).last().click();
  const calls = await page.evaluate(() => (window as unknown as { __desktopTestCalls: { command: string; args: { action?: unknown } }[] }).__desktopTestCalls);
  expect(calls.find(call => call.command === 'start_job')?.args.action).toEqual({ kind: 'unlock_vault', path: '/synthetic/vault', passphrase: 'synthetic testing passphrase' });
  await expect(page.getByRole('dialog')).toHaveCount(0);
});

test('untrusted names render as text and locking clears the archive', async ({ page }) => {
  await mockDesktop(page, true, true);
  await page.goto('/');
  await expect(page.getByRole('cell', { name: '<img src=x', exact: false })).toBeVisible();
  expect(await page.locator('img').count()).toBe(0);
  expect(await page.evaluate(() => (window as unknown as { untrustedRan?: boolean }).untrustedRan)).toBeUndefined();
  await page.getByRole('button', { name: 'Lock vault', exact: true }).click();
  await expect(page.getByRole('cell', { name: '<img src=x', exact: false })).toHaveCount(0);
  await expect(page.getByText('Your archive stays private', { exact: true })).toBeVisible();
});

test('capture uses verified readiness and sends a typed local job', async ({ page }) => {
  await mockDesktop(page, true, true);
  await page.goto('/');
  await page.getByRole('button', { name: 'Capture repository', exact: true }).first().click();
  await page.getByLabel('Snapshot name', { exact: true }).fill('synthetic-history');
  await page.getByLabel('Local repository', { exact: true }).fill('/synthetic/source');
  await page.getByRole('button', { name: 'Capture repository', exact: true }).last().click();
  const calls = await page.evaluate(() => (window as unknown as { __desktopTestCalls: { command: string; args: { action?: unknown } }[] }).__desktopTestCalls);
  expect(calls.find(call => call.command === 'start_job')?.args.action).toEqual({ kind: 'capture_local', name: 'synthetic-history', source: '/synthetic/source', includeLfs: false });
});

test('navigation and dialogs stay usable at the minimum desktop width', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 760 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Backups', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Backups', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Verify backup', exact: true }).click();
  await expect(page.getByRole('dialog')).toBeVisible();
  const box = await page.getByRole('dialog').boundingBox();
  expect(box?.x).toBeGreaterThanOrEqual(0);
  expect((box?.x || 0) + (box?.width || 0)).toBeLessThanOrEqual(900);
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});


test('private HTTPS capture submits transient credentials and a custom CA', async ({ page }) => {
  await mockDesktop(page, true, true);
  await page.goto('/');
  await page.getByRole('button', { name: 'Capture repository', exact: true }).first().click();
  await page.getByRole('button', { name: 'HTTPS remote', exact: true }).click();
  await page.getByLabel('Snapshot name', { exact: true }).fill('remote-history');
  await page.getByLabel('HTTPS clone URL', { exact: true }).fill('https://git.example.invalid/group/repo.git');
  await page.getByRole('button', { name: 'Username & token', exact: true }).click();
  await page.getByLabel('Repository username', { exact: true }).fill('synthetic-user');
  await page.getByLabel('Repository access token', { exact: true }).fill('synthetic-private-token');
  await page.getByLabel('Custom CA file (optional)', { exact: true }).fill('/synthetic/ca.pem');
  await page.getByRole('button', { name: 'Capture repository', exact: true }).last().click();
  const calls = await page.evaluate(() => (window as unknown as { __desktopTestCalls: { command: string; args: { action?: unknown } }[] }).__desktopTestCalls);
  expect(calls.find(call => call.command === 'start_job')?.args.action).toEqual({ kind: 'capture_remote', name: 'remote-history', url: 'https://git.example.invalid/group/repo.git', username: 'synthetic-user', token: 'synthetic-private-token', caFile: '/synthetic/ca.pem', includeLfs: false });
  await expect(page.getByRole('dialog')).toHaveCount(0);
  expect(await page.evaluate(() => localStorage.length + sessionStorage.length)).toBe(0);
  expect(await page.content()).not.toContain('synthetic-private-token');
});


test('migration planning sends transient scoped credentials without any apply action', async ({ page }) => {
  await mockDesktop(page, true, true);
  await page.goto('/');
  await page.getByRole('button', {name:'Migration plans',exact:true}).click();
  await page.getByLabel('Plan source snapshot').fill('history');
  await page.getByLabel('Destination provider').selectOption('gitlab');
  await page.getByLabel('Self-hosted instance').check();
  await page.getByLabel('Destination instance').fill('https://git.example/platform');
  await page.getByLabel('Destination repository', {exact:true}).fill('group/sub/repo');
  await page.getByLabel('Destination access token').fill('synthetic-plan-secret');
  await page.getByLabel('Destination Git username').fill('fixture');
  await page.getByLabel('Destination CA bundle', {exact:true}).fill('/synthetic/ca.pem');
  await page.getByRole('button',{name:'Prepare read-only plan'}).click();
  await expect(page.getByLabel('Destination access token')).toHaveValue('');
  const calls=await page.evaluate(()=>(window as unknown as {__desktopTestCalls:{command:string;args:{action?:unknown}}[]}).__desktopTestCalls);
  expect(calls.find(call=>call.command==='start_job')?.args.action).toEqual({kind:'prepare_plan',name:'history',provider:'gitlab',instance:'https://git.example/platform',repository:'group/sub/repo',token:'synthetic-plan-secret',username:'fixture',caFile:'/synthetic/ca.pem'});
  expect(await page.evaluate(()=>localStorage.length+sessionStorage.length)).toBe(0);
  expect(await page.content()).not.toContain('synthetic-plan-secret');
});

test('plan refs paginate, unknown policies stay visible, and locking clears metadata', async ({ page }) => {
  await mockDesktop(page,true,true,true);
  await page.goto('/');
  await page.getByRole('button',{name:'Migration plans',exact:true}).click();
  await expect(page.getByRole('cell',{name:'refs/heads/branch-0',exact:true})).toBeVisible();
  await expect(page.getByText('Coverage incomplete / unknown',{exact:false})).toBeVisible();
  await expect(page.getByText('Write authorization unproven',{exact:true})).toBeVisible();
  await page.getByRole('button',{name:'Next',exact:true}).click();
  await expect(page.getByRole('cell',{name:'refs/heads/branch-20',exact:true})).toBeVisible();
  await expect(page.getByRole('cell',{name:'refs/heads/branch-0',exact:true})).toHaveCount(0);
  await page.getByRole('button',{name:'Lock vault',exact:true}).click();
  await expect(page.getByRole('region',{name:'Migration plan result'})).toHaveCount(0);
  await expect(page.getByRole('button',{name:'Prepare read-only plan'})).toBeDisabled();
});


test('new private migration preparation clears credentials and uses typed IPC',async({page})=>{
 await mockDesktop(page,true,true);await page.goto('/');
 await page.getByRole('button',{name:'New migrations',exact:true}).click();
 await page.getByLabel('New migration source',{exact:true}).evaluate((node,value)=>{
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!.set!.call(node,value);
  node.dispatchEvent(new Event('input',{bubbles:true}));node.dispatchEvent(new Event('change',{bubbles:true}));
 },'history');
 await page.getByLabel('New migration repository',{exact:true}).fill('fixture/new-private');
 await page.getByLabel('New migration token',{exact:true}).fill('synthetic-new-token');
 await page.getByRole('button',{name:'Prepare private migration',exact:true}).click();
 await expect(page.getByLabel('New migration token',{exact:true})).toHaveValue('');
 const calls=await page.evaluate(()=>(window as unknown as {__desktopTestCalls:{command:string;args:{action?:unknown}}[]}).__desktopTestCalls);
 expect(calls.find(c=>c.command==='start_job')?.args.action).toEqual({kind:'prepare_new_migration',name:'history',provider:'github',instance:null,repository:'fixture/new-private',token:'synthetic-new-token',username:null,caFile:null});
 expect(await page.evaluate(()=>localStorage.length+sessionStorage.length)).toBe(0);
});

test('execution requires exact reviewed target and locking removes its review',async({page})=>{
 await mockDesktop(page,true,true,false,true);await page.goto('/');
 await page.getByRole('button',{name:'New migrations',exact:true}).click();
 await expect(page.getByRole('region',{name:'New private migration review'})).toContainText('https://github.com/');
 const execute=page.getByRole('button',{name:'Create private repository and migrate',exact:true});
 await expect(execute).toBeDisabled();
 await page.getByLabel('Execute migration token',{exact:true}).fill('synthetic-new-token');
 await page.getByLabel('Confirm new repository',{exact:true}).fill('fixture/existing');await expect(execute).toBeDisabled();
 await page.getByLabel('Confirm new repository',{exact:true}).fill('fixture/new-private');await execute.click();
 await expect(page.getByLabel('Execute migration token',{exact:true})).toHaveValue('');
 const calls=await page.evaluate(()=>(window as unknown as {__desktopTestCalls:{command:string;args:{action?:unknown}}[]}).__desktopTestCalls);
 expect(calls.find(c=>c.command==='start_job')?.args.action).toEqual({kind:'execute_new_migration',planId:'1'.repeat(32),confirmation:'fixture/new-private',token:'synthetic-new-token',username:null,caFile:null});
 await page.getByRole('button',{name:'Lock vault',exact:true}).click();
 await expect(page.getByRole('region',{name:'New private migration review'})).toHaveCount(0);
});

test('LFS capture is explicit and sent through typed IPC', async ({ page }) => {
  await mockDesktop(page, true, true);
  await page.goto('/');
  await page.getByRole('button', { name: 'Capture repository', exact: true }).first().click();
  await page.getByLabel('Snapshot name').fill('lfs-history');
  await page.getByLabel('Local repository', { exact: true }).fill('/synthetic/source');
  await page.getByLabel('Include Git LFS payloads', { exact: true }).check();
  await page.getByRole('dialog').getByRole('button', { name: 'Capture repository', exact: true }).click();
  const calls = await page.evaluate(() => (window as unknown as { __desktopTestCalls: { command: string; args: { action?: unknown } }[] }).__desktopTestCalls);
  expect(calls.find(call => call.command === 'start_job')?.args.action).toEqual({ kind:'capture_local',name:'lfs-history',source:'/synthetic/source',includeLfs:true });
});

test('private capture keeps submit visible in a short window', async ({ page }) => {
  await mockDesktop(page, true, true);
  await page.setViewportSize({width:900,height:620});
  await page.goto('/');
  await page.getByRole('button',{name:'Capture repository',exact:true}).first().click();
  await page.getByRole('button',{name:'HTTPS remote',exact:true}).click();
  await page.getByRole('button',{name:'Username & token',exact:true}).click();
  const submit=page.getByRole('dialog').getByRole('button',{name:'Capture repository',exact:true});
  const rect=await submit.boundingBox();
  expect(rect).not.toBeNull();
  expect(rect!.y).toBeGreaterThanOrEqual(0);
  expect(rect!.y+rect!.height).toBeLessThanOrEqual(620);
  await page.getByLabel('Include Git LFS payloads',{exact:true}).check();
  await expect(submit).toBeInViewport();
});


test('recovered migration receipts paginate, reconcile read-only and clear on lock', async ({page})=>{
  await mockDesktop(page,true,true);
  await page.goto('/');
  await page.getByRole('button',{name:'Migration history',exact:true}).click();
  const list=page.getByRole('region',{name:'Saved migrations'});
  await expect(list.getByRole('button',{name:'Inspect migration',exact:false})).toHaveCount(20);
  await page.getByRole('button',{name:'Next migrations',exact:true}).click();
  await expect(list.getByRole('button',{name:'Inspect migration',exact:false})).toHaveCount(1);
  const id=(21).toString(16).padStart(32,'0');
  await page.getByRole('button',{name:`Inspect migration ${id}`,exact:true}).click();
  const detail=page.getByRole('region',{name:'Saved migration details'});
  await expect(detail).toContainText('recovered-source');
  await expect(detail).toContainText('interrupted needs attention');
  await page.getByLabel('Reconciliation token',{exact:true}).fill('synthetic-read-token');
  await page.getByLabel('Reconciliation CA',{exact:true}).fill('/synthetic/ca.pem');
  await page.getByRole('button',{name:'Observe destination and save encrypted report',exact:true}).click();
  await expect(page.getByLabel('Reconciliation token',{exact:true})).toHaveValue('');
  await expect(page.getByLabel('Saved reconciliation reports')).toContainText('default branch differs');
  await expect(detail).toContainText('interrupted needs attention');
  const calls=await page.evaluate(()=>(window as unknown as {__desktopTestCalls:{command:string;args:{action?:unknown}}[]}).__desktopTestCalls);
  expect(calls.filter(c=>c.command==='start_job').map(c=>c.args.action)).toEqual([
    {kind:'inspect_migration',planId:id},
    {kind:'reconcile_migration',planId:id,token:'synthetic-read-token',username:null,caFile:'/synthetic/ca.pem'},
  ]);
  await page.getByRole('button',{name:'Lock vault',exact:true}).click();
  await expect(detail).toHaveCount(0);
  await expect(page.getByText('Unlock your vault to read migration history',{exact:true})).toBeVisible();
  await expect(page.getByText('recovered-source',{exact:false})).toHaveCount(0);
});

test('repository cards open restore and preselect a migration source',async({page})=>{
 await mockDesktop(page,true,true);
 await page.goto('/');
 await page.getByRole('button',{name:'Repositories',exact:true}).click();
 const library=page.getByRole('region',{name:'Repository library'});
 await expect(library.getByRole('article')).toHaveCount(1);
 await library.getByRole('button',{name:'Restore',exact:true}).click();
 await expect(page.getByRole('dialog')).toBeVisible();
 await page.keyboard.press('Escape');
 await library.getByRole('button',{name:'Migrate',exact:true}).click();
 await expect(page.getByLabel('New migration source',{exact:true})).toHaveValue('<img src=x onerror="window.untrustedRan=true">');
 await expect(page.locator('img')).toHaveCount(0);
});

test('snapshot browser selects an archived name and clears suggestions on lock',async({page})=>{
 await mockDesktop(page,true,true);
 await page.goto('/');
 await page.getByRole('button',{name:'New migrations',exact:true}).click();
 await page.getByRole('button',{name:'Browse saved snapshots',exact:true}).click();
 const suggestions=page.getByLabel('Saved snapshot suggestions',{exact:true});
 await expect(suggestions).toContainText('1 archived items');
 await suggestions.getByRole('button',{name:'<img src=x',exact:false}).click();
 await expect(page.getByLabel('New migration source',{exact:true})).toHaveValue('<img src=x onerror="window.untrustedRan=true">');
 await expect(suggestions).toHaveCount(0);
 await page.getByRole('button',{name:'Browse saved snapshots',exact:true}).click();
 await expect(suggestions).toBeVisible();
 await page.getByRole('button',{name:'Lock vault',exact:true}).click();
 await expect(suggestions).toHaveCount(0);
 await expect(page.getByLabel('New migration source',{exact:true})).toHaveValue('');
});

test('first-run home offers vault setup without enabling capture',async({page})=>{
 await page.goto('/');
 await page.getByRole('button',{name:'Create your vault',exact:true}).click();
 await expect(page.getByRole('dialog',{name:'Create an encrypted vault'})).toBeVisible();
 await page.keyboard.press('Escape');
 await expect(page.getByRole('button',{name:'Capture repository',exact:true}).first()).toBeDisabled();
});

test('capture suggests a repository name and preserves a custom label',async({page})=>{
 await mockDesktop(page,true,true);
 await page.goto('/');
 await page.getByRole('button',{name:'Capture repository',exact:true}).first().click();
 await page.getByLabel('Local repository',{exact:true}).fill('/synthetic/my-project.git');
 await page.getByLabel('Snapshot name',{exact:true}).focus();
 await expect(page.getByLabel('Snapshot name',{exact:true})).toHaveValue(/^my-project-\d{14}$/);
 await page.getByLabel('Snapshot name',{exact:true}).fill('my-release-archive');
 await page.getByLabel('Local repository',{exact:true}).fill('/synthetic/another.git');
 await page.getByLabel('Snapshot name',{exact:true}).focus();
 await expect(page.getByLabel('Snapshot name',{exact:true})).toHaveValue('my-release-archive');
});


test('guided Linux setup creates a new encrypted container with transient passphrases',async({page})=>{
 await mockDesktop(page,true);
 await page.goto('/');
 await page.getByRole('button',{name:'Set up workspace',exact:true}).click();
 await expect(page.getByRole('dialog',{name:'A protected place to work'})).toBeVisible();
 await expect(page.getByLabel('Protected storage file',{exact:true})).toHaveValue('/synthetic/working-storage.rsworkspace');
 await page.getByLabel('Storage passphrase',{exact:true}).fill('synthetic storage passphrase');
 await page.getByLabel('Confirm storage passphrase',{exact:true}).fill('different synthetic secret');
 await page.getByRole('button',{name:'Create and use protected storage',exact:true}).click();
 await expect(page.getByRole('alert')).toContainText('do not match');
 await page.getByLabel('Confirm storage passphrase',{exact:true}).fill('synthetic storage passphrase');
 await page.getByLabel('Protected storage capacity',{exact:true}).selectOption('8');
 await page.getByRole('button',{name:'Create and use protected storage',exact:true}).click();
 await expect(page.getByRole('dialog')).toHaveCount(0);
 const calls=await page.evaluate(()=>(window as unknown as {__desktopTestCalls:{command:string;args:{action?:unknown}}[]}).__desktopTestCalls);
 expect(calls.find(c=>c.command==='start_job')?.args.action).toEqual({kind:'create_protected_workspace',imagePath:'/synthetic/working-storage.rsworkspace',capacityGib:8,passphrase:'synthetic storage passphrase'});
});

test('guided setup reopens images and keeps existing-folder setup available',async({page})=>{
 await mockDesktop(page,true);
 await page.goto('/');
 await page.getByRole('button',{name:'Set up workspace',exact:true}).click();
 await page.getByRole('button',{name:'Unlock an existing container',exact:false}).click();
 await page.getByLabel('Storage passphrase',{exact:true}).fill('synthetic existing secret');
 await expect(page.getByLabel('Confirm storage passphrase',{exact:true})).toHaveCount(0);
 await page.getByRole('button',{name:'Use an existing encrypted folder',exact:true}).click();
 await expect(page.getByRole('dialog',{name:'Set up Git working storage'})).toBeVisible();
 await page.getByRole('button',{name:'Create workspace',exact:true}).click();
 await expect(page.getByLabel('Encrypted workspace',{exact:true})).toBeVisible();
});

test('Windows offers one-click EFS setup without a container password',async({page})=>{
 await mockDesktop(page,true,false,false,false,'windows');
 await page.goto('/');
 await page.getByRole('button',{name:'Set up workspace',exact:true}).click();
 await expect(page.getByText('Let Windows protect the folder',{exact:true})).toBeVisible();
 await expect(page.getByLabel('Storage passphrase',{exact:true})).toHaveCount(0);
 await page.getByRole('button',{name:'Create encrypted working folder',exact:true}).click();
 const calls=await page.evaluate(()=>(window as unknown as {__desktopTestCalls:{command:string;args:{action?:unknown}}[]}).__desktopTestCalls);
 expect(calls.find(c=>c.command==='start_job')?.args.action).toEqual({kind:'create_default_workspace'});
});

test('JetBrains Mono is bundled and the storage wizard fits a short window',async({page})=>{
 await mockDesktop(page,true);
 await page.setViewportSize({width:900,height:700});
 await page.goto('/');
 await expect.poll(()=>page.evaluate(()=>document.fonts.check('14px "JetBrains Mono"'))).toBe(true);
 expect(await page.evaluate(()=>getComputedStyle(document.body).fontFamily)).toContain('JetBrains Mono');
 await page.getByRole('button',{name:'Set up workspace',exact:true}).click();
 const submit=page.getByRole('button',{name:'Create and use protected storage',exact:true});
 await expect(submit).toBeVisible();
 const box=await submit.boundingBox();expect((box?.y??0)+(box?.height??0)).toBeLessThanOrEqual(700);
 await page.keyboard.press('Escape');await expect(page.getByRole('dialog')).toHaveCount(0);
});
