import { expect, test, type Page } from '@playwright/test';

async function mockDesktop(page: Page, unlocked = false, workspace = false) {
  await page.addInitScript(({ unlocked, workspace }) => {
    const state = { version: '0.1.0-alpha.3', platform: 'linux', vault: unlocked ? { path: '/synthetic/vault', itemCount: 1, totalBytes: 4096 } : null, workspace: workspace ? { path: '/synthetic/luks/work', encryption: 'LinuxLuks' } : null, busy: false, jobs: [] as unknown[], remoteCapture: false };
    const calls: { command: string; args: unknown }[] = [];
    Object.assign(window, { __desktopTestCalls: calls, __TAURI_INTERNALS__: { invoke: async (command: string, args: unknown) => {
      calls.push({ command, args });
      if (command === 'get_status') return state;
      if (command === 'get_items') return { items: [{ name: '<img src=x onerror="window.untrustedRan=true">', size: 4096, sha256: 'a'.repeat(64) }], total: 1 };
      if (command === 'lock_vault') { state.vault = null; return; }
      if (command === 'start_job') return 1;
      if (command === 'choose_path') return '/synthetic/chosen';
      throw new Error('Unexpected test command: ' + command);
    } } });
  }, { unlocked, workspace });
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
  expect(calls.find(call => call.command === 'start_job')?.args.action).toEqual({ kind: 'capture_local', name: 'synthetic-history', source: '/synthetic/source' });
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
