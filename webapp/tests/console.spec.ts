import { expect, test } from '@playwright/test';
import { webToken } from './e2e-home';

test.describe('agentvault console', () => {
  test('bootstrap token signs in and is stripped from the URL', async ({ page }) => {
    await page.goto(`/?token=${encodeURIComponent(webToken())}`);

    await expect(page.locator('.sidebar')).toBeVisible();
    await expect(page.getByTestId('session-tenant')).toContainText('default');
    await expect(page).not.toHaveURL(/[?&]token=/);
  });

  test('login, create secret, issue ticket, pick agent path, filter audit', async ({ page }) => {
    await page.goto('/');

    await page.getByTestId('login-token').fill(webToken());
    await page.getByTestId('login-local').click();

    await expect(page.getByTestId('session-tenant')).toContainText('default');
    await expect(page.locator('.sidebar')).toBeVisible();

    const name = `e2e/secret-${Date.now()}`;
    await page.locator('a[href="#/secrets"]').click();
    await page.getByTestId('btn-new-secret').click();
    await page.getByTestId('new-secret-name').fill(name);
    await page.getByTestId('new-secret-value').fill('e2e-value');
    await page.getByTestId('btn-create-secret').click();
    await expect(page.locator('.cell-main', { hasText: name })).toBeVisible();

    await page.locator('a[href="#/tickets"]').click();
    await page.getByTestId('btn-issue-ticket').click();
    await page.getByTestId('issue-secret').selectOption(name);
    await page.getByTestId('issue-ttl').fill('15m');
    await page.getByTestId('btn-issue-confirm').click();
    await expect(page.getByTestId('ticket-issued')).toBeVisible();
    await page.getByRole('button', { name: '完成' }).click();

    await page.locator('a[href="#/agents"]').click();
    await page.getByTestId('agent-pick-claude').click();
    await expect(page.getByTestId('agent-path-picker')).toBeVisible();
    await page.getByTestId('agent-path-cancel').click();

    await page.locator('a[href="#/audit"]').click();
    const rows = page.getByTestId('audit-row');
    await expect(rows.first()).toBeVisible();

    await page.getByTestId('audit-filter-issue').click();
    const filtered = await rows.count();
    expect(filtered).toBeGreaterThan(0);
    for (let index = 0; index < filtered; index += 1) {
      await expect(rows.nth(index).locator('.tl-event')).toContainText('issue');
    }
  });
});