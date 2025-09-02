import { test, expect } from '@playwright/test';

test.describe('Settings User Flow', () => {
  test.beforeEach(async ({ page }) => {
    // Assuming user is logged in and has access to settings
    await page.goto('/settings/tokens'); // Navigate to API Tokens page
  });

  test('should display API tokens and allow creation', async ({ page }) => {
    await expect(page.locator('h1')).toContainText('API Tokens'); // Adjust selector and text
    await expect(page.locator('[data-testid="api-token-list"]')).toBeVisible(); // Adjust selector for token list

    await page.click('button:has-text("Generate New Token")'); // Adjust selector
    await expect(page.locator('h2')).toContainText('New API Token'); // Adjust modal title

    const tokenName = `Test Token ${Date.now()}`;
    await page.fill('input[name="token-name"]', tokenName); // Adjust selector
    await page.click('button:has-text("Create Token")'); // Adjust selector

    await expect(page.locator('[data-testid="success-notification"]')).toBeVisible();
    await expect(page.locator('[data-testid="success-notification"]')).toContainText('Token created successfully');

    await expect(page.locator(`text=${tokenName}`)).toBeVisible(); // Verify new token in list
  });

  test('should allow revoking an API token', async ({ page }) => {
    // Assuming a token exists or is created in a beforeAll/beforeEach hook
    // Find a token and click revoke
    await page.locator('[data-testid="api-token-item"]').first().locator('button:has-text("Revoke")').click(); // Adjust selectors

    // Confirm revocation in a modal/dialog
    await page.click('button:has-text("Confirm Revoke")'); // Adjust selector

    await expect(page.locator('[data-testid="success-notification"]')).toBeVisible();
    await expect(page.locator('[data-testid="success-notification"]')).toContainText('Token revoked successfully');

    // Verify token is no longer in the list
  });

  test('should display ABAC policies and allow editing (if applicable)', async ({ page }) => {
    await page.goto('/settings/policies'); // Navigate to ABAC Policies page
    await expect(page.locator('h1')).toContainText('ABAC Policies'); // Adjust selector and text
    await expect(page.locator('[data-testid="policy-list"]')).toBeVisible(); // Adjust selector

    // Test editing a policy (if UI allows)
    // await page.locator('[data-testid="policy-item"]').first().locator('button:has-text("Edit")').click();
    // ... interact with code editor ...
  });

  // Add tests for other settings like system configuration
});
