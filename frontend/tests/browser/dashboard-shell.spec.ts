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
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
});
