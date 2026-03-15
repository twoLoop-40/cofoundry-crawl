/**
 * SPA-aware Visual QA Crawler — auto-discovers routes from navigation,
 * or uses a provided route list, then visits each after login.
 *
 * Usage:
 *   npx tsx src/spa-crawl.ts <url> [options]
 *
 * Options:
 *   --output=DIR           Output directory (default: ./output)
 *   --routes=FILE          JSON route list (array of {path, name} or strings)
 *   --auth-email=STR       Login email
 *   --auth-password=STR    Login password
 *   --login-url=STR        Login page URL (default: <origin>/login)
 *   --no-prompt            Disable UI credential prompt
 *   --timeout=MS           Per-page timeout (default: 30000)
 *   --detail-pattern=STR   Detail page URL pattern (e.g., "/items/{id}")
 *   --api-pattern=STR      API intercept URL patterns, comma-separated (default: /api/,/auth/)
 *
 * Route discovery (when --routes not provided):
 *   1. Navigate to startUrl after login
 *   2. Scan nav/sidebar links (<nav a[href]>, sidebar a[href])
 *   3. Deduplicate by pathname → generate route list
 */

import { chromium, type Page, type BrowserContext } from 'playwright';
import * as fs from 'fs';
import * as path from 'path';
import { resolveCredentials, saveCredentials } from './credential-prompt';

// ── CLI args ────────────────────────────────────────────────────────────
const args = process.argv.slice(2);
const startUrl = args.find(a => !a.startsWith('--'));

if (!startUrl) {
  console.error('Usage: npx tsx src/spa-crawl.ts <url> [options]');
  console.error('  Example: npx tsx src/spa-crawl.ts http://localhost:3000');
  process.exit(1);
}

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
const ROUTES_FILE = getArg('routes', '');
const DETAIL_PATTERN = getArg('detail-pattern', '');
const API_PATTERNS = getArg('api-pattern', '/api/,/auth/').split(',').filter(Boolean);

// ── Types ───────────────────────────────────────────────────────────────
interface SpaRoute {
  path: string;
  name: string;
}

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

// ── Route Discovery ─────────────────────────────────────────────────────

/** Auto-discover routes from navigation elements on the page */
async function discoverRoutes(page: Page, origin: string): Promise<SpaRoute[]> {
  console.log('🔍 Auto-discovering routes from navigation...');

  const links = await page.evaluate(`(() => {
    const seen = new Set();
    const routes = [];

    // Scan nav elements, sidebar, and role="navigation"
    const navSelectors = [
      'nav a[href]',
      '[role="navigation"] a[href]',
      'aside a[href]',
      '[class*="sidebar"] a[href]',
      '[class*="Sidebar"] a[href]',
      '[data-testid*="nav"] a[href]',
      '[data-testid*="sidebar"] a[href]',
    ];

    for (const sel of navSelectors) {
      document.querySelectorAll(sel).forEach(el => {
        const href = el.getAttribute('href');
        if (!href || href === '#' || href.startsWith('http')) return;

        // Normalize: strip trailing slash, skip hash-only
        const pathname = href.split('?')[0].split('#')[0].replace(/\\/$/, '') || '/';
        if (seen.has(pathname)) return;
        seen.add(pathname);

        // Derive name from pathname
        const name = pathname === '/'
          ? 'home'
          : pathname.slice(1).replace(/\\//g, '-').replace(/[^a-zA-Z0-9-]/g, '');

        routes.push({ path: pathname, name: name || 'page' });
      });
    }

    return routes;
  })()`) as SpaRoute[];

  // Ensure root is included
  if (!links.find(r => r.path === '/')) {
    links.unshift({ path: '/', name: 'home' });
  }

  console.log(`  Found ${links.length} routes:`);
  links.forEach(r => console.log(`    ${r.name} → ${r.path}`));

  return links;
}

/** Load routes from a JSON file */
function loadRoutesFromFile(filePath: string): SpaRoute[] {
  const content = fs.readFileSync(filePath, 'utf-8');
  const data = JSON.parse(content);

  // Support both array of {path, name} and array of strings
  if (Array.isArray(data)) {
    return data.map((item: any) => {
      if (typeof item === 'string') {
        const name = item === '/' ? 'home' : item.slice(1).replace(/\//g, '-');
        return { path: item, name };
      }
      return { path: item.path, name: item.name || item.path.slice(1).replace(/\//g, '-') || 'home' };
    });
  }

  throw new Error(`Invalid routes file format. Expected array, got ${typeof data}`);
}

// ── Detail Page Discovery ───────────────────────────────────────────────

/** Discover detail page links matching a pattern like "/items/{id}" */
async function discoverDetailPages(
  context: BrowserContext,
  origin: string,
  pattern: string,
  timeout: number,
): Promise<SpaRoute[]> {
  // Convert pattern to regex: "/items/{id}" → /^\/items\/([^/]+)$/
  const regexStr = pattern.replace(/\{[^}]+\}/g, '([^/]+)');
  const regex = new RegExp(`^${regexStr}$`);

  // Find the list page (route that is the prefix of the pattern)
  const listPath = pattern.replace(/\/\{[^}]+\}.*$/, '');

  console.log(`\n🔍 Discovering detail pages matching: ${pattern}`);
  const page = await context.newPage();
  await page.goto(`${origin}${listPath}`, { waitUntil: 'networkidle', timeout });
  await page.waitForTimeout(2000);

  const hrefs = await page.evaluate(`(() => {
    return Array.from(document.querySelectorAll('a[href], [onclick], tr[class*="cursor"], [role="link"]'))
      .map(el => {
        const href = el.getAttribute('href');
        if (href) return href;
        const onclick = el.getAttribute('onclick') || '';
        const match = onclick.match(/(?:href|navigate|push).*?["']([^"']+)["']/);
        return match ? match[1] : null;
      })
      .filter(Boolean);
  })()`) as string[];

  const matches = hrefs
    .filter(href => regex.test(href.split('?')[0]))
    .slice(0, 3); // Max 3 detail pages

  await page.close();

  if (matches.length > 0) {
    console.log(`  Found ${matches.length} detail pages`);
    return matches.map((href, i) => ({
      path: href,
      name: `detail-${i + 1}`,
    }));
  }

  console.log('  No detail pages found');
  return [];
}

// ── Main ────────────────────────────────────────────────────────────────
async function crawl() {
  const origin = new URL(startUrl).origin;

  // ── Credential Resolution ──
  let projectRoot = process.cwd();
  for (let dir = projectRoot; dir !== path.dirname(dir); dir = path.dirname(dir)) {
    if (fs.existsSync(path.join(dir, '.env')) || fs.existsSync(path.join(dir, 'package.json'))) {
      projectRoot = dir;
      break;
    }
  }

  let credsFromPrompt = false;
  if (!AUTH_EMAIL || !AUTH_PASSWORD) {
    const creds = await resolveCredentials({
      projectRoot,
      targetUrl: startUrl,
      noPrompt: NO_PROMPT,
    });
    if (creds) {
      AUTH_EMAIL = creds.email;
      AUTH_PASSWORD = creds.password;
      credsFromPrompt = true;
    }
  }

  // ── Resolve routes ──
  let spaRoutes: SpaRoute[];

  if (ROUTES_FILE) {
    spaRoutes = loadRoutesFromFile(ROUTES_FILE);
    console.log(`📋 Loaded ${spaRoutes.length} routes from ${ROUTES_FILE}`);
  } else {
    // Will auto-discover after login
    spaRoutes = [];
  }

  console.log(`\n🔍 SPA Visual QA Crawler`);
  console.log(`  Origin: ${origin}`);
  console.log(`  Routes: ${spaRoutes.length > 0 ? spaRoutes.length : 'auto-discover'}`);
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
    const loginUrl = getArg('login-url', '') || `${origin}/login`;
    console.log(`🔐 Logging in at ${loginUrl}...`);
    await loginPage.goto(loginUrl, { waitUntil: 'domcontentloaded', timeout: TIMEOUT });
    await loginPage.waitForTimeout(2000);

    const emailInput = loginPage.locator('input[type="email"], input[name="email"], input[name="username"]').first();
    const passInput = loginPage.locator('input[type="password"]').first();

    try {
      await emailInput.waitFor({ state: 'visible', timeout: 10000 });
      await emailInput.fill(AUTH_EMAIL);
      await passInput.fill(AUTH_PASSWORD);
      await loginPage.locator('button[type="submit"]').first().click();
      await loginPage.waitForTimeout(3000);

      const isLoggedIn = !loginPage.url().includes('/login');
      if (isLoggedIn) {
        console.log(`✅ Logged in (current: ${loginPage.url()})`);
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

  // Auto-discover routes if not provided
  if (spaRoutes.length === 0) {
    const discoverPage = await context.newPage();
    await discoverPage.goto(startUrl, { waitUntil: 'networkidle', timeout: TIMEOUT });
    await discoverPage.waitForTimeout(2000);
    spaRoutes = await discoverRoutes(discoverPage, origin);
    await discoverPage.close();
  }

  const pages: PageResult[] = [];

  for (const route of spaRoutes) {
    const url = route.path.startsWith('http') ? route.path : `${origin}${route.path}`;
    console.log(`📄 [${pages.length + 1}/${spaRoutes.length}] ${route.name} → ${url}`);

    const page = await context.newPage();
    const apiCalls: { method: string; url: string; status: number }[] = [];

    // Intercept API calls (configurable patterns)
    page.on('response', async (response) => {
      const reqUrl = response.url();
      if (API_PATTERNS.some(p => reqUrl.includes(p))) {
        apiCalls.push({
          method: response.request().method(),
          url: reqUrl,
          status: response.status(),
        });
      }
    });

    try {
      await page.goto(url, { waitUntil: 'networkidle', timeout: TIMEOUT });
      await page.waitForTimeout(2000);

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

  // Detail page discovery (if --detail-pattern provided)
  if (DETAIL_PATTERN) {
    const detailRoutes = await discoverDetailPages(context, origin, DETAIL_PATTERN, TIMEOUT);
    for (const route of detailRoutes) {
      const url = `${origin}${route.path}`;
      console.log(`📄 [detail] ${route.name} → ${url}`);

      const page = await context.newPage();
      const apiCalls: { method: string; url: string; status: number }[] = [];
      page.on('response', async (response) => {
        if (API_PATTERNS.some(p => response.url().includes(p))) {
          apiCalls.push({ method: response.request().method(), url: response.url(), status: response.status() });
        }
      });

      try {
        await page.goto(url, { waitUntil: 'networkidle', timeout: TIMEOUT });
        await page.waitForTimeout(2000);
        await page.screenshot({ path: path.join(OUTPUT_DIR, 'screenshots', `${route.name}.png`), fullPage: true });

        const domData = await page.evaluate(() => ({
          headings: Array.from(document.querySelectorAll('h1,h2,h3,h4')).map(el => ({ level: parseInt(el.tagName[1]), text: (el.textContent || '').trim().slice(0, 200) })),
          buttons: Array.from(document.querySelectorAll('button')).map(el => ({ text: (el.textContent || '').trim().slice(0, 100), type: (el as HTMLButtonElement).type || 'button' })),
          tables: Array.from(document.querySelectorAll('table')).map(t => ({ headers: Array.from(t.querySelectorAll('th')).map(th => (th.textContent || '').trim()), rowCount: t.querySelectorAll('tbody tr').length })),
        }));

        pages.push({ url, name: route.name, title: await page.title(), screenshot: `screenshots/${route.name}.png`, ...domData, apiCalls, errors: [] });
        console.log(`  ✅ captured`);
      } catch (err) {
        console.log(`  ❌ ${(err as Error).message}`);
      }
      await page.close();
    }
  }

  await browser.close();

  // Write result
  const result = {
    startUrl: origin,
    crawledAt: new Date().toISOString(),
    totalPages: pages.length,
    routeSource: ROUTES_FILE ? 'file' : 'auto-discover',
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
