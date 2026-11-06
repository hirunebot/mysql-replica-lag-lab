import { expect, test } from '@playwright/test';

test('未取得画面が横にはみ出さず、設定を開閉できる', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Master' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Slave' })).toBeVisible();
  await expect(page.getByText('処理の計画は未取得です。')).toBeVisible();
  await expect(page.getByRole('button', { name: '開始', exact: true })).toBeDisabled();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
  const settings = page.getByRole('button', { name: '設定', exact: true });
  await settings.click();
  await expect(page.getByRole('dialog', { name: '実行設定' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(settings).toBeFocused();
});

test('明示したfixtureで処理行を選択するとSQLを表示する', async ({ page }) => {
  await page.goto('/?fixture=1');
  await expect(page.getByText('サンプル / DB未接続')).toBeVisible();
  await page.getByRole('button', { name: 'UPDATE 2', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'UPDATE 2', exact: true })).toBeVisible();
  await expect(page.locator('pre')).toContainText('UPDATE lag_demo.items');
  await expect(page.getByText('バインド値')).toContainText('50001, 100000');
  await expect(page.getByRole('row').filter({ has: page.getByRole('button', { name: 'UPDATE 2', exact: true }) })).toContainText('ID 50,001–100,000');
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
});

test('ホバーとキーボードで目的を確認し、Escapeで閉じる', async ({ page, isMobile }) => {
  await page.goto('/?fixture=1');
  const operation = page.getByRole('button', { name: 'UPDATE 2', exact: true });
  if (!isMobile) {
    await operation.hover();
    await expect(page.getByRole('tooltip')).toContainText('指定のID範囲');
    await page.mouse.move(0, 0);
    await expect(page.getByRole('tooltip')).toHaveCount(0);
  }
  await operation.focus();
  await expect(page.getByRole('tooltip')).toContainText('指定のID範囲');
  await page.keyboard.press('Escape');
  await expect(page.getByRole('tooltip')).toHaveCount(0);
  await page.keyboard.press('Enter');
  await expect(page.getByRole('heading', { name: 'UPDATE 2', exact: true })).toBeVisible();
  await expect(operation).toBeFocused();
});
