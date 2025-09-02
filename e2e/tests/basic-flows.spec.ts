import { test, expect } from '@playwright/test';

test.describe('Basic Application Flows', () => {
  test.beforeEach(async ({ page }) => {
    // Ensure we start from a clean state for each test
    await page.goto('/login'); // Assuming login is the entry point
  });

  test('should allow a user to log in and log out successfully', async ({ page }) => {
    // Assuming default credentials for basic login
    await page.fill('input[name="email"]', 'admin@example.com'); // Replace with actual selectors/credentials
    await page.fill('input[name="password"]', 'password'); // Replace with actual selectors/credentials
    await page.click('button[type="submit"]');

    // Expect to be redirected to dashboard or home page
    await expect(page).toHaveURL('/'); // Adjust expected URL
    await expect(page.locator('h1')).toContainText('Dashboard'); // Adjust based on actual dashboard title

    // Log out
    await page.click('button[data-testid="logout-button"]'); // Adjust selector for logout button
    await expect(page).toHaveURL('/login'); // Adjust expected URL after logout
  });

  test('should display an error message for invalid login credentials', async ({ page }) => {
    await page.fill('input[name="email"]', 'invalid@example.com');
    await page.fill('input[name="password"]', 'wrongpassword');
    await page.click('button[type="submit"]');

    // Expect an error message to be visible
    await expect(page.locator('[data-testid="login-error-message"]')).toBeVisible(); // Adjust selector
    await expect(page.locator('[data-testid="login-error-message"]')).toContainText('Invalid credentials'); // Adjust text
  });

  test('should navigate to key pages successfully', async ({ page }) => {
    // First, log in to access protected routes
    await page.fill('input[name="email"]', 'admin@example.com');
    await page.fill('input[name="password"]', 'password');
    await page.click('button[type="submit"]');
    await expect(page).toHaveURL('/');

    // Navigate to Repositories page
    await page.click('a[href="/repositories"]'); // Adjust selector for navigation link
    await expect(page).toHaveURL('/repositories');
    await expect(page.locator('h1')).toContainText('Repositories');

    // Navigate to Search page
    await page.click('a[href="/search"]'); // Adjust selector for navigation link
    await expect(page).toHaveURL('/search');
    await expect(page.locator('h1')).toContainText('Advanced Search');
  });

  test('should allow basic repository creation and verification', async ({ page }) => {
    // Log in
    await page.fill('input[name="email"]', 'admin@example.com');
    await page.fill('input[name="password"]', 'password');
    await page.click('button[type="submit"]');
    await expect(page).toHaveURL('/');

    // Navigate to Repositories page
    await page.click('a[href="/repositories"]');
    await expect(page).toHaveURL('/repositories');

    // Click "New Repository" button
    await page.click('button:has-text("New Repository")'); // Adjust selector

    // Fill form
    const repoName = `test-repo-${Date.now()}`;
    await page.fill('input[name="name"]', repoName); // Adjust selector
    await page.selectOption('select[name="type"]', 'maven'); // Adjust selector and value
    await page.click('button:has-text("Create")'); // Adjust selector

    // Verify success message or redirection
    await expect(page.locator('[data-testid="success-notification"]')).toBeVisible(); // Adjust selector
    await expect(page.locator('[data-testid="success-notification"]')).toContainText('Repository created successfully'); // Adjust text

    // Verify the new repository appears in the list
    await expect(page.locator(`text=${repoName}`)).toBeVisible();
  });
});
