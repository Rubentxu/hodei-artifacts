import puppeteer from 'puppeteer';

async function testRoutes() {
  const browser = await puppeteer.launch({ headless: true });
  const page = await browser.newPage();

  // Test home route
  console.log('Testing / route...');
  await page.goto('http://localhost:5173/', { waitUntil: 'networkidle0' });
  const homeTitle = await page.title();
  console.log('Home page title:', homeTitle);

  // Test login route
  console.log('Testing /login route...');
  await page.goto('http://localhost:5173/login', { waitUntil: 'networkidle0' });
  const loginTitle = await page.title();
  console.log('Login page title:', loginTitle);

  // Test dashboard route
  console.log('Testing /dashboard route...');
  await page.goto('http://localhost:5173/dashboard', {
    waitUntil: 'networkidle0',
  });
  const dashboardTitle = await page.title();
  console.log('Dashboard page title:', dashboardTitle);

  // Test 404 route
  console.log('Testing non-existent route...');
  await page.goto('http://localhost:5173/non-existent', {
    waitUntil: 'networkidle0',
  });
  const notFoundTitle = await page.title();
  console.log('404 page title:', notFoundTitle);

  await browser.close();
  console.log('All routes tested successfully!');
}

testRoutes().catch(console.error);
