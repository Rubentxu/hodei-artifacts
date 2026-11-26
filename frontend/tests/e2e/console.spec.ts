/**
 * The console, end to end, against the real service.
 *
 * What is measured here is that the console reads the catalog of the
 * service, that it says what the service says and that it asks before it
 * changes what the service serves.
 */

import { readFileSync } from 'node:fs';

import { expect, test, type Page } from '@playwright/test';

import { theAssetPath, theDigest, theRepository } from './fixture/seed.mjs';
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
 *
 * The credential goes into the store that the application reads on boot,
 * which is where a person who has signed in would have it. The console
 * asks the API with it, and the API answers as it answers for anybody.
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

test.describe('the console of the catalog', () => {
  test.beforeEach(async ({ page }) => {
    await signIn(page);
  });

  test('names the repositories of the account', async ({ page }) => {
    await page.goto('/console/repositories');

    await expect(
      page.getByRole('link', { name: theRepository, exact: true })
    ).toBeVisible();
    await expect(page.getByText('Raw', { exact: true })).toBeVisible();
    await expect(page.getByText('Hosted', { exact: true })).toBeVisible();
  });

  test('offers only the modalities that the service serves', async ({
    page,
  }) => {
    await page.goto('/console/repositories');
    await page.getByRole('button', { name: 'New repository' }).click();

    // The options of a select are not visible while it is closed: what
    // is measured is that the format is offered.
    await expect(page.getByRole('option', { name: 'Raw' })).toHaveCount(1);
    await expect(page.getByRole('option', { name: 'OCI' })).toHaveCount(1);
    // Helm is declared by the release but no surface serves it yet.
    await expect(page.getByRole('option', { name: 'Helm' })).toHaveCount(0);

    // Cargo is served in a single modality, OCI in three.
    await page.getByRole('combobox').selectOption('cargo');
    await expect(page.getByRole('radio')).toHaveCount(1);
    await page.getByRole('combobox').selectOption('oci');
    await expect(page.getByRole('radio')).toHaveCount(3);
  });

  test('finds the asset of the catalog', async ({ page }) => {
    await page.goto('/console/browse');

    await expect(
      page.getByRole('link', { name: theAssetPath, exact: true })
    ).toBeVisible();
    await expect(page.getByText('1 result', { exact: false })).toBeVisible();
  });

  test('says what the asset is', async ({ page }) => {
    await page.goto('/console/browse');
    await page.getByRole('link', { name: theAssetPath, exact: true }).click();

    await expect(
      page.getByText(`sha256:${theDigest.split(':')[1]}`).first()
    ).toBeVisible();
    await expect(page.getByText('Identity')).toBeVisible();
    await expect(page.getByText('Trusted', { exact: true })).toBeVisible();

    // The trust of the distribution lives in its own tab.
    await page.getByRole('tab', { name: 'Trust' }).click();
    await expect(
      page.getByRole('heading', { name: 'Distribution trust' })
    ).toBeVisible();
  });

  test('asks for a reason before it revokes a concession', async ({
    page,
  }) => {
    await page.goto(`/console/packages/${theRepository}/${theAssetPath}`);

    await page.getByRole('button', { name: 'Revoke' }).click();

    await expect(page.getByText('Revoke the concession')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Revoke' })).toBeDisabled();

    await page.getByLabel('Reason').fill('the signature does not hold');
    await expect(page.getByRole('button', { name: 'Revoke' })).toBeEnabled();
  });

  test('downloads the bytes of the asset', async ({ page }) => {
    await page.goto(`/console/packages/${theRepository}/${theAssetPath}`);

    const answer = page.waitForResponse(
      response => response.url().includes('/repository/')
    );
    await page.getByRole('button', { name: 'Download' }).click();

    const response = await answer;
    expect(response.status()).toBe(200);
    expect(await response.text()).toContain('hodei artifacts console fixture');
  });
});
