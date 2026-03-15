/**
 * SPA-aware Visual QA Crawler — directly visits known routes after login.
 * For React Router SPAs where navigation is via buttons, not <a href>.
 */

import { chromium, type Page, type BrowserContext } from 'playwright';
import * as fs from 'fs';
import * as path from 'path';
import { resolveCredentials, saveCredentials } from './credential-prompt';

// ── CLI args ────────────────────────────────────────────────────────────
const args = process.argv.slice(2);
const startUrl = args.find(a => !a.startsWith('--')) || 'https://safe-intelligence.vercel.app';

function getArg(name: string, defaultVal: string): string {
  const found = args.find(a => a.startsWith(`--${name}=`));
  return found ? found.split('=').slice(1).join('=') : defaultVal;
}
function hasFlag(name: string): boolean {
  return args.includes(`--${name}`);
}

const OUTPUT_DIR = getArg('output', './output');
let AUTH_EMAIL = getArg('auth-email', '');
let AUTH_PASSWORD = getArg('auth-password', '');
const NO_PROMPT = hasFlag('no-prompt');
const TIMEOUT = parseInt(getArg('timeout', '30000'));

// ── Known SPA routes ────────────────────────────────────────────────────
const SPA_ROUTES = [
  { path: '/', name: 'overview' },
  { path: '/domains', name: 'domains' },
  { path: '/findings', name: 'findings' },
  { path: '/dark-web', name: 'dark-web' },
  { path: '/scan/new', name: 'scan-new' },
  { path: '/scan/history', name: 'scan-history' },
  { path: '/intelligence', name: 'intelligence' },
  { path: '/settings', name: 'settings' },
];

// ── Types ───────────────────────────────────────────────────────────────
interface PageResult {
  url: string;
  name: string;
  title: string;
  screenshot: string;
  headings: { level: number; text: string }[];
  buttons: { text: string; type: string }[];
  tables: { headers: string[]; rowCount: number }[];
  apiCalls: { method: string; url: string; status: number }[];
  errors: string[];
}

// ── Main ────────────────────────────────────────────────────────────────
async function crawl() {
  const origin = new URL(startUrl).origin;

  // ── Credential Resolution (범용) ──
  // Find project root (walk up from cwd to find .env or package.json)
  let projectRoot = process.cwd();
  for (let dir = projectRoot; dir !== path.dirname(dir); dir = path.dirname(dir)) {
    if (fs.existsSync(path.join(dir, '.env')) || fs.existsSync(path.join(dir, 'package.json'))) {
      projectRoot = dir;
      break;
    }
  }

  let credsFromPrompt = false;  // true = UI에서 입력받음, 로그인 성공 시 .env에 저장
  if (!AUTH_EMAIL || !AUTH_PASSWORD) {
    const creds = await resolveCredentials({
      projectRoot,
      targetUrl: startUrl,
      noPrompt: NO_PROMPT,
    });
    if (creds) {
      AUTH_EMAIL = creds.email;
      AUTH_PASSWORD = creds.password;
      credsFromPrompt = true;  // .env에서 찾았으면 resolveCredentials 내부에서 이미 로그 출력
    }
  }

  console.log(`\n🔍 SPA Visual QA Crawler`);
  console.log(`  Origin: ${origin}`);
  console.log(`  Routes: ${SPA_ROUTES.length}`);
  console.log(`  Auth: ${AUTH_EMAIL ? AUTH_EMAIL : 'none'}`);
  console.log(`  Output: ${OUTPUT_DIR}\n`);

  fs.mkdirSync(path.join(OUTPUT_DIR, 'screenshots'), { recursive: true });

  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    ignoreHTTPSErrors: true,
  });

  // Login
  if (AUTH_EMAIL && AUTH_PASSWORD) {
    const loginPage = await context.newPage();
    const loginUrl = `${origin}/login`;
    console.log(`🔐 Logging in at ${loginUrl}...`);
    await loginPage.goto(loginUrl, { waitUntil: 'domcontentloaded', timeout: TIMEOUT });
    await loginPage.waitForTimeout(2000);

    const emailInput = loginPage.locator('input[type="email"], input[name="email"]').first();
    const passInput = loginPage.locator('input[type="password"]').first();

    try {
      await emailInput.waitFor({ state: 'visible', timeout: 10000 });
      await emailInput.fill(AUTH_EMAIL);
      await passInput.fill(AUTH_PASSWORD);
      await loginPage.locator('button[type="submit"]').first().click();
      await loginPage.waitForTimeout(3000);

      // Login success check: not on login page anymore
      const isLoggedIn = !loginPage.url().includes('/login');
      if (isLoggedIn) {
        console.log(`✅ Logged in (current: ${loginPage.url()})`);
        // Save to .env ONLY after login succeeds + credentials came from UI prompt
        if (credsFromPrompt) {
          saveCredentials(projectRoot, AUTH_EMAIL, AUTH_PASSWORD);
        }
        console.log('');
      } else {
        console.log(`⚠️ Login may have failed (still on login page)\n`);
      }
    } catch (err) {
      console.log(`⚠️ Login failed: ${(err as Error).message}\n`);
    }
    await loginPage.close();
  }

  const pages: PageResult[] = [];

  for (const route of SPA_ROUTES) {
    const url = `${origin}${route.path}`;
    console.log(`📄 [${pages.length + 1}/${SPA_ROUTES.length}] ${route.name} → ${url}`);

    const page = await context.newPage();
    const apiCalls: { method: string; url: string; status: number }[] = [];

    // Intercept API calls
    page.on('response', async (response) => {
      const reqUrl = response.url();
      if (reqUrl.includes('/api/') || reqUrl.includes('/auth/')) {
        apiCalls.push({
          method: response.request().method(),
          url: reqUrl,
          status: response.status(),
        });
      }
    });

    try {
      await page.goto(url, { waitUntil: 'networkidle', timeout: TIMEOUT });
      await page.waitForTimeout(2000); // Wait for React render + API calls

      // Check if redirected to login (auth failed)
      if (page.url().includes('/login')) {
        console.log(`  ⚠️ Redirected to login — skipping`);
        await page.close();
        continue;
      }

      // Screenshot
      const screenshotPath = path.join(OUTPUT_DIR, 'screenshots', `${route.name}.png`);
      await page.screenshot({ path: screenshotPath, fullPage: true });

      // Extract DOM
      const domData = await page.evaluate(() => {
        const headings = Array.from(document.querySelectorAll('h1, h2, h3, h4, h5, h6')).map(el => ({
          level: parseInt(el.tagName[1]),
          text: (el.textContent || '').trim().slice(0, 200),
        }));

        const buttons = Array.from(document.querySelectorAll('button, [role="button"]')).map(el => ({
          text: (el.textContent || '').trim().slice(0, 100),
          type: (el as HTMLButtonElement).type || 'button',
        }));

        const tables = Array.from(document.querySelectorAll('table')).map(table => {
          const headers = Array.from(table.querySelectorAll('th')).map(th => (th.textContent || '').trim());
          const rows = table.querySelectorAll('tbody tr');
          return { headers, rowCount: rows.length };
        });

        return { headings, buttons, tables };
      });

      pages.push({
        url,
        name: route.name,
        title: await page.title(),
        screenshot: `screenshots/${route.name}.png`,
        headings: domData.headings,
        buttons: domData.buttons,
        tables: domData.tables,
        apiCalls,
        errors: [],
      });

      console.log(`  ✅ ${domData.headings.length} headings, ${domData.tables.length} tables, ${apiCalls.length} API calls`);
    } catch (err) {
      console.log(`  ❌ Error: ${(err as Error).message}`);
      pages.push({
        url,
        name: route.name,
        title: '',
        screenshot: '',
        headings: [],
        buttons: [],
        tables: [],
        apiCalls: [],
        errors: [(err as Error).message],
      });
    }

    await page.close();
  }

  // Also try to discover domain detail pages from the domains list
  if (pages.find(p => p.name === 'domains')) {
    console.log(`\n🔍 Discovering domain detail pages...`);
    const discoverPage = await context.newPage();
    await discoverPage.goto(`${origin}/domains`, { waitUntil: 'networkidle', timeout: TIMEOUT });
    await discoverPage.waitForTimeout(2000);

    // Get domain IDs from the page
    const domainLinks = await discoverPage.evaluate(() => {
      // Look for clickable domain rows that might have data attributes or links
      const rows = document.querySelectorAll('[data-testid*="domain"], tr[class*="cursor-pointer"]');
      const ids: string[] = [];
      rows.forEach(row => {
        const onclick = row.getAttribute('onclick') || '';
        const match = onclick.match(/domains\/([a-f0-9-]+)/);
        if (match) ids.push(match[1]);
      });
      return ids;
    });

    if (domainLinks.length > 0) {
      const firstDomain = domainLinks[0];
      const url = `${origin}/domains/${firstDomain}`;
      console.log(`📄 [extra] domain-detail → ${url}`);

      const page = await context.newPage();
      const apiCalls: { method: string; url: string; status: number }[] = [];
      page.on('response', async (response) => {
        if (response.url().includes('/api/')) {
          apiCalls.push({ method: response.request().method(), url: response.url(), status: response.status() });
        }
      });

      try {
        await page.goto(url, { waitUntil: 'networkidle', timeout: TIMEOUT });
        await page.waitForTimeout(2000);
        await page.screenshot({ path: path.join(OUTPUT_DIR, 'screenshots', 'domain-detail.png'), fullPage: true });

        const domData = await page.evaluate(() => ({
          headings: Array.from(document.querySelectorAll('h1,h2,h3,h4')).map(el => ({ level: parseInt(el.tagName[1]), text: (el.textContent||'').trim().slice(0,200) })),
          buttons: Array.from(document.querySelectorAll('button')).map(el => ({ text: (el.textContent||'').trim().slice(0,100), type: (el as HTMLButtonElement).type||'button' })),
          tables: Array.from(document.querySelectorAll('table')).map(t => ({ headers: Array.from(t.querySelectorAll('th')).map(th => (th.textContent||'').trim()), rowCount: t.querySelectorAll('tbody tr').length })),
        }));

        pages.push({ url, name: 'domain-detail', title: await page.title(), screenshot: 'screenshots/domain-detail.png', ...domData, apiCalls, errors: [] });
        console.log(`  ✅ captured`);
      } catch (err) {
        console.log(`  ❌ ${(err as Error).message}`);
      }
      await page.close();
    }

    await discoverPage.close();
  }

  await browser.close();

  // Write result
  const result = {
    startUrl: origin,
    crawledAt: new Date().toISOString(),
    totalPages: pages.length,
    pages,
  };

  const outPath = path.join(OUTPUT_DIR, 'crawl-result.json');
  fs.writeFileSync(outPath, JSON.stringify(result, null, 2));

  console.log(`\n✅ Crawl complete: ${pages.length} pages`);
  console.log(`📁 Results: ${outPath}`);
  console.log(`🖼️  Screenshots: ${path.join(OUTPUT_DIR, 'screenshots/')}`);
}

crawl().catch(err => {
  console.error('Fatal:', err);
  process.exit(1);
});
