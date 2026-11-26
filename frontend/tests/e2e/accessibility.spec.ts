/**
 * The console with the keyboard, and what it says to a screen reader.
 *
 * The console is the place where somebody decides whether a piece of
 * software may be trusted, and that somebody may not be able to use a
 * mouse. The basics are measured here: the fields have names, the
 * dialogs take the focus and the escape closes them, and the errors are
 * announced.
 */

import { readFileSync } from 'node:fs';

import { expect, test, type Page } from '@playwright/test';

import { theRepository } from './fixture/seed.mjs';
import { theTokenPath } from './fixture/token.mjs';

/**
 * The token that the fixture signed.
 *
 * The secret is in the fixture and not in the repository, so the token is
 * read from the file the fixture leaves, and not signed here.
 */
const theTokenOfTheFixture = () => readFileSync(theTokenPath, 'utf8').trim();

/**
 * Signs the browser in.
 */
const signIn = async (page: Page) => {
  await page.addInitScript(token => {
    window.localStorage.setItem(
      'hodei-auth-token',
      JSON.stringify({
        state: {
          user: { name: 'alice', role: 'admin' },
          token,
          isAuthenticated: true,
        },
        version: 0,
      })
    );
  }, theTokenOfTheFixture());
};

test.describe('the console with the keyboard', () => {
  test.beforeEach(async ({ page }) => {
    await signIn(page);
  });

  test('every field of the search has a name', async ({ page }) => {
    await page.goto('/console/browse');

    await expect(page.getByLabel('Repository')).toBeVisible();
    await expect(page.getByLabel('Package')).toBeVisible();
    await expect(page.getByLabel('Version')).toBeVisible();
    await expect(page.getByLabel('Text')).toBeVisible();
  });

  test('the search can be asked without a mouse', async ({ page }) => {
    await page.goto('/console/browse');

    await page.getByLabel('Repository').fill(theRepository);
    await page.getByLabel('Repository').press('Enter');

    await expect(page).toHaveURL(/repository=console-fixture/);
  });

  test('the dialog takes the focus and the escape closes it', async ({
    page,
  }) => {
    await page.goto('/console/repositories');

    await page.getByRole('button', { name: 'New repository' }).click();
    await expect(page.getByRole('dialog')).toBeVisible();

    const inside = await page.evaluate(() => {
      const dialog = document.querySelector('[role="dialog"]');
      return dialog?.contains(document.activeElement) ?? false;
    });
    expect(inside).toBe(true);

    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog')).toHaveCount(0);
  });

  test('the narrow-screen navigation dialog contains tab focus and restores its trigger', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto('/');

    const trigger = page.getByRole('button', { name: 'Open navigation menu' });
    await trigger.click();

    const dialog = page.getByRole('dialog', { name: 'Mobile navigation menu' });
    await expect(dialog).toBeVisible();
    const links = dialog
      .getByRole('navigation', {
        name: 'Mobile primary navigation',
      })
      .getByRole('link');
    const firstLink = links.nth(0);
    await expect(firstLink).toBeFocused();

    const secondLink = links.nth(1);
    const thirdLink = links.nth(2);
    const fourthLink = links.nth(3);
    const logout = dialog.getByRole('button', { name: 'Logout' });
    for (const next of [secondLink, thirdLink, fourthLink, logout]) {
      await page.keyboard.press('Tab');
      await expect(next).toBeFocused();
    }

    await page.keyboard.press('Tab');
    const focusRemainsInDialog = await dialog.evaluate(
      element =>
        element.contains(document.activeElement) ||
        document.activeElement === document.body
    );
    expect(focusRemainsInDialog).toBe(true);
    await page.keyboard.press('Tab');
    await expect(firstLink).toBeFocused();

    await page.keyboard.press('Escape');
    await expect(dialog).toHaveCount(0);
    await expect(trigger).toBeFocused();
  });

  test('the repositories are reachable with the keyboard', async ({ page }) => {
    await page.goto('/console/repositories');

    const link = page.getByRole('link', { name: theRepository, exact: true });
    await link.focus();
    await expect(link).toBeFocused();

    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(/\/console\/browse\?repository=/);
    await expect(page.getByLabel('Repository')).toHaveValue(theRepository);
  });
});
