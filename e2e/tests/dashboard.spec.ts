import { test, expect } from '@playwright/test';

test.describe('Dashboard User Flow', () => {
  test.beforeEach(async ({ page }) => {
    // Assuming user is already logged in for dashboard access
    // You might want to add a login step here if not handled by global setup
    await page.goto('/'); // Navigate to the dashboard
  });

  test('should load the dashboard and display key metrics', async ({ page }) => {
    await expect(page.locator('h1')).toContainText('Dashboard'); // Adjust selector and text

    // Verify presence of key metric cards (adjust selectors and expected texts)
    await expect(page.locator('[data-testid="total-packages-card"]')).toBeVisible();
    await expect(page.locator('[data-testid="active-repositories-card"]')).toBeVisible();
    await expect(page.locator('[data-testid="online-users-card"]')).toBeVisible();
    await expect(page.locator('[data-testid="storage-used-card"]')).toBeVisible();

    // Verify some content within the cards (e.g., a number)
    await expect(page.locator('[data-testid="total-packages-value"]')).not.toBeEmpty();
  });

  test('should display recent repositories and allow navigation', async ({ page }) => {
    // Verify recent repositories section is visible
    await expect(page.locator('h2:has-text("Recent Repositories")')).toBeVisible(); // Adjust selector

    // Verify at least one recent repository is listed (adjust selector)
    await expect(page.locator('[data-testid="recent-repository-item"]').first()).toBeVisible();

    // Click on a recent repository and verify navigation (adjust selector)
    await page.locator('[data-testid="recent-repository-item"]').first().click();
    await expect(page).toHaveURL(/.+\/repositories\/.+/); // Expect to navigate to a repository detail page
  });

  test('should display recent activity feed', async ({ page }) => {
    // Verify recent activity section is visible
    await expect(page.locator('h2:has-text("Recent Activity")')).toBeVisible(); // Adjust selector

    // Verify at least one activity item is listed (adjust selector)
    await expect(page.locator('[data-testid="activity-feed-item"]').first()).toBeVisible();
    await expect(page.locator('[data-testid="activity-feed-item"]').first()).not.toBeEmpty();
  });
});
