/**
 * Visual QA Crawler — BFS site crawl with screenshots + DOM + API capture.
 *
 * Usage:
 *   npx tsx src/index.ts <url> [--depth=3] [--max-pages=50] [--output=./output]
 *     [--auth-email=...] [--auth-password=...] [--login-url=...]
 */

import { chromium, type Page, type BrowserContext, type Route } from 'playwright';
import * as fs from 'fs';
import * as path from 'path';

// ── CLI args ────────────────────────────────────────────────────────────

const args = process.argv.slice(2);
const startUrl = args.find(a => !a.startsWith('--')) || 'http://localhost:5173';

function getArg(name: string, defaultVal: string): string {
  const found = args.find(a => a.startsWith(`--${name}=`));
  return found ? found.split('=').slice(1).join('=') : defaultVal;
}

const MAX_DEPTH = parseInt(getArg('depth', '3'));
const MAX_PAGES = parseInt(getArg('max-pages', '50'));
const OUTPUT_DIR = getArg('output', './output');
const AUTH_EMAIL = getArg('auth-email', '');
const AUTH_PASSWORD = getArg('auth-password', '');
const LOGIN_URL = getArg('login-url', '');
const TIMEOUT = parseInt(getArg('timeout', '30000'));

// ── Types ───────────────────────────────────────────────────────────────

interface PageResult {
  url: string;
  title: string;
  depth: number;
  screenshot: string;
  headings: { level: number; text: string }[];
  links: { href: string; text: string }[];
  buttons: { text: string; type: string }[];
  forms: { action: string; method: string; fields: { name: string; type: string; required: boolean }[] }[];
  tables: { headers: string[]; rowCount: number }[];
  apiCalls: ApiCall[];
  errors: string[];
}

interface ApiCall {
  method: string;
  url: string;
  status: number;
  requestBody?: unknown;
  responseBody?: unknown;
  timestamp: number;
}

interface CrawlResult {
  startUrl: string;
  crawledAt: string;
  totalPages: number;
  pages: PageResult[];
  siteMap: { url: string; links: string[] }[];
  apiEndpoints: { method: string; path: string; count: number }[];
}

// ── Helpers ─────────────────────────────────────────────────────────────

function normalizeUrl(url: string, base: string): string | null {
  try {
    const parsed = new URL(url, base);
    // Only same origin
    const origin = new URL(base).origin;
    if (parsed.origin !== origin) return null;
    // Strip hash
    parsed.hash = '';
    return parsed.href;
  } catch {
    return null;
  }
}

function slugify(url: string): string {
  return url
    .replace(/^https?:\/\//, '')
    .replace(/[^a-zA-Z0-9]/g, '_')
    .slice(0, 80);
}

// ── Main crawler ────────────────────────────────────────────────────────

async function crawl(): Promise<CrawlResult> {
  console.log(`\n🔍 Visual QA Crawler`);
  console.log(`  URL: ${startUrl}`);
  console.log(`  Depth: ${MAX_DEPTH}, Max pages: ${MAX_PAGES}`);
  console.log(`  Output: ${OUTPUT_DIR}\n`);

  fs.mkdirSync(path.join(OUTPUT_DIR, 'screenshots'), { recursive: true });

  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    ignoreHTTPSErrors: true,
  });

  // API call interceptor
  const apiCalls: Map<string, ApiCall[]> = new Map();

  // Login if credentials provided
  if (AUTH_EMAIL && AUTH_PASSWORD) {
    const loginPage = await context.newPage();
    const loginUrl = LOGIN_URL || `${new URL(startUrl).origin}/login`;
    console.log(`🔐 Logging in at ${loginUrl}...`);
    await loginPage.goto(loginUrl, { waitUntil: 'domcontentloaded', timeout: TIMEOUT });
    await loginPage.waitForTimeout(2000); // Wait for React hydration

    // Try common login form patterns
    const emailInput = loginPage.locator('input[type="email"], input[name="email"], input[name="username"], input#email').first();
    const passInput = loginPage.locator('input[type="password"]').first();

    try {
      await emailInput.waitFor({ state: 'visible', timeout: 10000 });
      await passInput.waitFor({ state: 'visible', timeout: 5000 });

      await emailInput.fill(AUTH_EMAIL);
      await passInput.fill(AUTH_PASSWORD);

      // Submit form
      const submitBtn = loginPage.locator('button[type="submit"]').first();
      await submitBtn.click();
      await loginPage.waitForTimeout(3000); // Wait for login API + redirect
      console.log(`✅ Logged in successfully (current: ${loginPage.url()})\n`);
    } catch (err) {
      console.log(`⚠️ Login form not found or login failed: ${(err as Error).message}\n`);
    }
    await loginPage.close();
  }

  // BFS crawl
  const visited = new Set<string>();
  const queue: { url: string; depth: number }[] = [{ url: startUrl, depth: 0 }];
  const pages: PageResult[] = [];
  const siteMap: { url: string; links: string[] }[] = [];
  const allApiCalls: ApiCall[] = [];

  while (queue.length > 0 && pages.length < MAX_PAGES) {
    const { url, depth } = queue.shift()!;
    if (visited.has(url) || depth > MAX_DEPTH) continue;
    visited.add(url);

    console.log(`📄 [${pages.length + 1}/${MAX_PAGES}] depth=${depth} ${url}`);

    const page = await context.newPage();
    const pageApiCalls: ApiCall[] = [];

    // Intercept API calls
    page.on('response', async (response) => {
      const reqUrl = response.url();
      if (reqUrl.includes('/api/') || reqUrl.includes('/auth/')) {
        try {
          const call: ApiCall = {
            method: response.request().method(),
            url: reqUrl,
            status: response.status(),
            timestamp: Date.now(),
          };
          // Capture response body for JSON
          const contentType = response.headers()['content-type'] || '';
          if (contentType.includes('json')) {
            try {
              call.responseBody = await response.json();
            } catch { /* ignore */ }
          }
          // Capture request body
          const postData = response.request().postData();
          if (postData) {
            try {
              call.requestBody = JSON.parse(postData);
            } catch {
              call.requestBody = postData;
            }
          }
          pageApiCalls.push(call);
        } catch { /* ignore */ }
      }
    });

    try {
      await page.goto(url, { waitUntil: 'networkidle', timeout: TIMEOUT });
      // Wait a bit for dynamic content
      await page.waitForTimeout(1500);

      // Screenshot
      const screenshotName = `${slugify(url)}.png`;
      const screenshotPath = path.join(OUTPUT_DIR, 'screenshots', screenshotName);
      await page.screenshot({ path: screenshotPath, fullPage: true });

      // Extract DOM structure
      const domData = await page.evaluate(() => {
        // Headings
        const headings = Array.from(document.querySelectorAll('h1, h2, h3, h4, h5, h6')).map(el => ({
          level: parseInt(el.tagName[1]),
          text: (el.textContent || '').trim().slice(0, 200),
        }));

        // Links
        const links = Array.from(document.querySelectorAll('a[href]')).map(el => ({
          href: (el as HTMLAnchorElement).href,
          text: (el.textContent || '').trim().slice(0, 100),
        })).filter(l => l.href && !l.href.startsWith('javascript:'));

        // Buttons
        const buttons = Array.from(document.querySelectorAll('button, [role="button"]')).map(el => ({
          text: (el.textContent || '').trim().slice(0, 100),
          type: (el as HTMLButtonElement).type || 'button',
        }));

        // Forms
        const forms = Array.from(document.querySelectorAll('form')).map(form => ({
          action: (form as HTMLFormElement).action || '',
          method: (form as HTMLFormElement).method || 'get',
          fields: Array.from(form.querySelectorAll('input, select, textarea')).map(field => ({
            name: (field as HTMLInputElement).name || '',
            type: (field as HTMLInputElement).type || 'text',
            required: (field as HTMLInputElement).required || false,
          })),
        }));

        // Tables
        const tables = Array.from(document.querySelectorAll('table')).map(table => {
          const headers = Array.from(table.querySelectorAll('th')).map(th => (th.textContent || '').trim());
          const rows = table.querySelectorAll('tbody tr');
          return { headers, rowCount: rows.length };
        });

        return { headings, links, buttons, forms, tables };
      });

      // Collect links for BFS
      const childLinks: string[] = [];
      for (const link of domData.links) {
        const normalized = normalizeUrl(link.href, url);
        if (normalized && !visited.has(normalized)) {
          childLinks.push(normalized);
          if (depth + 1 <= MAX_DEPTH) {
            queue.push({ url: normalized, depth: depth + 1 });
          }
        }
      }

      siteMap.push({ url, links: childLinks });

      pages.push({
        url,
        title: await page.title(),
        depth,
        screenshot: `screenshots/${screenshotName}`,
        headings: domData.headings,
        links: domData.links,
        buttons: domData.buttons,
        forms: domData.forms,
        tables: domData.tables,
        apiCalls: pageApiCalls,
        errors: [],
      });

      allApiCalls.push(...pageApiCalls);
    } catch (err) {
      console.log(`  ❌ Error: ${(err as Error).message}`);
      pages.push({
        url,
        title: '',
        depth,
        screenshot: '',
        headings: [],
        links: [],
        buttons: [],
        forms: [],
        tables: [],
        apiCalls: [],
        errors: [(err as Error).message],
      });
    }

    await page.close();
  }

  await browser.close();

  // Aggregate API endpoints
  const endpointMap = new Map<string, number>();
  for (const call of allApiCalls) {
    const key = `${call.method} ${new URL(call.url).pathname}`;
    endpointMap.set(key, (endpointMap.get(key) || 0) + 1);
  }
  const apiEndpoints = Array.from(endpointMap.entries()).map(([key, count]) => {
    const [method, p] = key.split(' ');
    return { method, path: p, count };
  });

  const result: CrawlResult = {
    startUrl,
    crawledAt: new Date().toISOString(),
    totalPages: pages.length,
    pages,
    siteMap,
    apiEndpoints,
  };

  // Write result
  const outputPath = path.join(OUTPUT_DIR, 'crawl-result.json');
  fs.writeFileSync(outputPath, JSON.stringify(result, null, 2));
  console.log(`\n✅ Crawl complete: ${pages.length} pages`);
  console.log(`📁 Results: ${outputPath}`);
  console.log(`🖼️  Screenshots: ${path.join(OUTPUT_DIR, 'screenshots/')}\n`);

  return result;
}

crawl().catch(err => {
  console.error('Crawl failed:', err);
  process.exit(1);
});
