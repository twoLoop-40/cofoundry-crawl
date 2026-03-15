/**
 * Click Crawl — 클릭 가능한 요소를 찾아 전후 스크린샷 + 상태 변화 기록
 *
 * Usage:
 *   npx tsx src/click-crawl.ts <url> [options]
 *
 * Options:
 *   --output=DIR           결과 디렉토리 (default: ./output-click)
 *   --targets=SEL1,SEL2    클릭할 CSS 셀렉터 (콤마 구분). 미지정 시 자동 탐색
 *   --depth=N              클릭 깊이 (default: 1)
 *   --max-clicks=N         최대 클릭 횟수 (default: 30)
 *   --viewport-width=N     뷰포트 너비 (default: 1440)
 *   --viewport-height=N    뷰포트 높이 (default: 900)
 *   --wait=N               클릭 후 대기 ms (default: 3000)
 *   --auth                 로그인 필요 시
 *   --auth-email=STR       로그인 이메일
 *   --auth-password=STR    로그인 비밀번호
 *   --login-url=STR        로그인 페이지 URL
 *   --no-navigate          페이지 이동 시 클릭 스킵 (인라인 변화만)
 *   --no-restore           각 클릭 후 원래 페이지로 돌아가지 않음
 */
import { chromium, type Page, type Locator } from 'playwright';
import * as fs from 'fs';
import * as path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// ── Types ──
interface ClickTarget {
  selector: string;
  label: string;
  tag: string;
  href?: string;
  ariaExpanded?: string;
  testId?: string;
}

interface ClickResult {
  index: number;
  selector: string;
  label: string;
  type: 'navigation' | 'expand' | 'action' | 'unknown';
  before: {
    url: string;
    screenshot: string;
    ariaExpanded?: string;
  };
  after: {
    url: string;
    screenshot: string;
    urlChanged: boolean;
    ariaExpanded?: string;
    newTestIds: string[];
    error?: string;
  };
  params: Record<string, string | null>;
  duration: number;
}

// ── CLI args ──
function parseArgs() {
  const args = process.argv.slice(2);
  const opts: Record<string, string> = {};
  const positional: string[] = [];

  for (const arg of args) {
    if (arg.startsWith('--')) {
      const [key, ...rest] = arg.slice(2).split('=');
      opts[key] = rest.join('=') || 'true';
    } else {
      positional.push(arg);
    }
  }

  const url = positional[0];
  if (!url) {
    console.error('Usage: npx tsx src/click-crawl.ts <url> [options]');
    process.exit(1);
  }

  return {
    url,
    output: path.resolve(opts['output'] ?? './output-click'),
    targets: opts['targets']?.split(',').filter(Boolean) ?? [],
    depth: parseInt(opts['depth'] ?? '1'),
    maxClicks: parseInt(opts['max-clicks'] ?? '30'),
    viewportWidth: parseInt(opts['viewport-width'] ?? '1440'),
    viewportHeight: parseInt(opts['viewport-height'] ?? '900'),
    wait: parseInt(opts['wait'] ?? '3000'),
    auth: opts['auth'] === 'true',
    authEmail: opts['auth-email'] ?? '',
    authPassword: opts['auth-password'] ?? '',
    loginUrl: opts['login-url'] ?? '',
    noNavigate: opts['no-navigate'] === 'true',
    restore: opts['no-restore'] !== 'true',
  };
}

// ── Login ──
async function login(page: Page, origin: string, opts: ReturnType<typeof parseArgs>) {
  const loginUrl = opts.loginUrl || `${origin}/login`;
  console.log(`🔑 Logging in at ${loginUrl}...`);
  await page.goto(loginUrl, { waitUntil: 'networkidle', timeout: 30000 });

  const emailInput = page.locator('input[type="email"], input[name="email"], input[name="username"]').first();
  const passInput = page.locator('input[type="password"]').first();

  if (await emailInput.isVisible({ timeout: 3000 }).catch(() => false)) {
    await emailInput.fill(opts.authEmail);
    await passInput.fill(opts.authPassword);
    await page.click('button[type="submit"]');
    await page.waitForURL('**/', { timeout: 15000 }).catch(() => {});
    await page.waitForTimeout(3000);
    console.log('✅ Logged in');
  } else {
    console.log('⚠️ Login form not found, continuing without auth');
  }
}

// ── Auto-discover clickable elements ──
// Uses string-based evaluate for serialization safety (avoids bundler name injection issues)
async function discoverTargets(page: Page): Promise<ClickTarget[]> {
  return page.evaluate(`(() => {
    const seen = new Set();
    const targets = [];

    const addTarget = (el, selector) => {
      if (seen.has(selector)) return;
      const rect = el.getBoundingClientRect();
      if (rect.width < 10 || rect.height < 10) return;
      if (getComputedStyle(el).display === 'none') return;
      if (getComputedStyle(el).visibility === 'hidden') return;

      seen.add(selector);
      targets.push({
        selector,
        label: (el.textContent || '').trim().slice(0, 60),
        tag: el.tagName.toLowerCase(),
        href: el.getAttribute('href') || undefined,
        ariaExpanded: el.getAttribute('aria-expanded') || undefined,
        testId: el.getAttribute('data-testid') || undefined,
      });
    };

    // Priority 1: data-testid buttons and links
    document.querySelectorAll('[data-testid] button, [data-testid] a, button[data-testid], a[data-testid]').forEach((el) => {
      const testId = el.getAttribute('data-testid') || (el.closest('[data-testid]') ? el.closest('[data-testid]').getAttribute('data-testid') : null);
      if (testId) addTarget(el, '[data-testid="' + testId + '"] ' + el.tagName.toLowerCase());
    });

    // Priority 2: Nav items
    document.querySelectorAll('nav a, nav button').forEach((el) => {
      const href = el.getAttribute('href');
      if (href) {
        addTarget(el, 'nav a[href="' + href + '"]');
      } else {
        const text = (el.textContent || '').trim().slice(0, 30);
        if (text) addTarget(el, 'nav button:has-text("' + text + '")');
      }
    });

    // Priority 3: aria-expanded toggles
    document.querySelectorAll('button[aria-expanded]').forEach((el) => {
      const parent = el.closest('[data-testid]');
      const testId = parent ? parent.getAttribute('data-testid') : null;
      if (testId) addTarget(el, '[data-testid="' + testId + '"] button[aria-expanded]');
    });

    // Priority 4: Table rows with data-testid
    document.querySelectorAll('tr[data-testid]').forEach((el) => {
      const testId = el.getAttribute('data-testid');
      if (testId) addTarget(el, '[data-testid="' + testId + '"]');
    });

    // Priority 5: Standalone buttons (not in nav)
    document.querySelectorAll('main button, [role="main"] button').forEach((el, i) => {
      const text = (el.textContent || '').trim().slice(0, 30);
      if (text && text.length > 1 && !el.closest('nav')) {
        addTarget(el, 'main button:nth-of-type(' + (i + 1) + ')');
      }
    });

    // Priority 6: Internal links (not nav, not external)
    document.querySelectorAll('main a[href^="/"], [role="main"] a[href^="/"]').forEach((el) => {
      const href = el.getAttribute('href');
      if (href && !el.closest('nav')) {
        addTarget(el, 'main a[href="' + href + '"]');
      }
    });

    // Priority 7: Generic clickable divs/spans with cursor pointer
    document.querySelectorAll('div[role="button"], span[role="button"], [onclick]').forEach((el, i) => {
      const text = (el.textContent || '').trim().slice(0, 30);
      if (text) addTarget(el, '[role="button"]:nth-of-type(' + (i + 1) + ')');
    });

    // Priority 8: Any anchor tag (Figma sites use generic links)
    document.querySelectorAll('a[href]').forEach((el) => {
      const href = el.getAttribute('href');
      if (href && href !== '#') {
        addTarget(el, 'a[href="' + href + '"]');
      }
    });

    return targets;
  })()`) as Promise<ClickTarget[]>;
}

// ── Get all data-testid values on page ──
// String-based evaluate to avoid esbuild __name injection (bug #35)
async function getTestIds(page: Page): Promise<string[]> {
  return page.evaluate(`
    Array.from(document.querySelectorAll('[data-testid]'))
      .map(el => el.getAttribute('data-testid'))
      .filter(Boolean)
  `) as Promise<string[]>;
}

// ── Get URL params ──
function getUrlParams(url: string): Record<string, string | null> {
  try {
    const u = new URL(url);
    const params: Record<string, string | null> = {};
    for (const [k, v] of u.searchParams) {
      params[k] = v;
    }
    return params;
  } catch {
    return {};
  }
}

// ── Classify click type ──
function classifyClick(before: { url: string; ariaExpanded?: string }, after: { url: string; ariaExpanded?: string }): ClickResult['type'] {
  if (before.url !== after.url) return 'navigation';
  if (before.ariaExpanded !== after.ariaExpanded) return 'expand';
  return 'unknown';
}

// ── Click one target ──
async function clickTarget(
  page: Page,
  target: ClickTarget,
  index: number,
  ssDir: string,
  opts: ReturnType<typeof parseArgs>,
): Promise<ClickResult | null> {
  const prefix = String(index).padStart(2, '0');
  const beforeUrl = page.url();

  // Screenshot before
  const beforeScreenshot = `${prefix}-before-${target.testId ?? 'el'}.png`;
  await page.screenshot({ path: path.join(ssDir, beforeScreenshot) });

  // Try to find the element
  let locator: Locator;
  try {
    // Try exact selector first
    locator = page.locator(target.selector).first();
    if (await locator.count() === 0) {
      console.log(`  ⚠️ ${prefix}: selector not found: ${target.selector}`);
      return null;
    }
  } catch {
    console.log(`  ⚠️ ${prefix}: invalid selector: ${target.selector}`);
    return null;
  }

  // Scroll into view
  await locator.scrollIntoViewIfNeeded({ timeout: 3000 }).catch(() => {});

  // Get before state
  const beforeAria = await locator.getAttribute('aria-expanded').catch(() => null);
  const beforeTestIds = await getTestIds(page);

  // Click
  const start = Date.now();
  try {
    await locator.click({ timeout: 5000 });
  } catch (err: any) {
    console.log(`  ⚠️ ${prefix}: click failed: ${err.message.slice(0, 80)}`);
    return null;
  }

  await page.waitForTimeout(opts.wait);
  const duration = Date.now() - start;

  const afterUrl = page.url();
  const urlChanged = beforeUrl !== afterUrl;

  // Skip if navigated away and --no-navigate
  if (urlChanged && opts.noNavigate) {
    console.log(`  ⏭️ ${prefix}: navigated away (skipped in no-navigate mode)`);
    if (opts.restore) {
      await page.goBack();
      await page.waitForTimeout(2000);
    }
    return null;
  }

  // Get after state
  const afterAria = await locator.getAttribute('aria-expanded').catch(() => null);
  const afterTestIds = await getTestIds(page);
  const newTestIds = afterTestIds.filter((id) => !beforeTestIds.includes(id));

  // Screenshot after
  const afterScreenshot = `${prefix}-after-${target.testId ?? 'el'}.png`;
  await page.screenshot({ path: path.join(ssDir, afterScreenshot) });

  const type = classifyClick(
    { url: beforeUrl, ariaExpanded: beforeAria ?? undefined },
    { url: afterUrl, ariaExpanded: afterAria ?? undefined },
  );

  const result: ClickResult = {
    index,
    selector: target.selector,
    label: target.label,
    type,
    before: {
      url: beforeUrl,
      screenshot: beforeScreenshot,
      ariaExpanded: beforeAria ?? undefined,
    },
    after: {
      url: afterUrl,
      screenshot: afterScreenshot,
      urlChanged,
      ariaExpanded: afterAria ?? undefined,
      newTestIds,
    },
    params: getUrlParams(afterUrl),
    duration,
  };

  const icon = type === 'navigation' ? '🔗' : type === 'expand' ? '📂' : '🔘';
  console.log(`  ${icon} ${prefix}: [${type}] ${target.label.slice(0, 40)} ${urlChanged ? `→ ${afterUrl}` : ''}`);

  // Restore if needed
  if (urlChanged && opts.restore) {
    await page.goBack();
    await page.waitForTimeout(2000);
  } else if (type === 'expand' && afterAria === 'true') {
    // Collapse back for next click
    await locator.click({ timeout: 3000 }).catch(() => {});
    await page.waitForTimeout(500);
  }

  return result;
}

// ── Process one page ──
async function processPage(
  page: Page,
  url: string,
  outDir: string,
  opts: ReturnType<typeof parseArgs>,
  currentDepth: number,
): Promise<ClickResult[]> {
  const ssDir = path.join(outDir, 'screenshots');
  fs.mkdirSync(ssDir, { recursive: true });

  console.log(`\n🖱️  Click crawl: ${url} (depth ${currentDepth})`);
  await page.goto(url, { waitUntil: 'networkidle', timeout: 60000 });
  await page.waitForTimeout(opts.wait);

  // Initial screenshot
  await page.screenshot({ path: path.join(ssDir, '00-initial.png'), fullPage: true });

  // Discover or use provided targets
  let targets: ClickTarget[];
  if (opts.targets.length > 0 && currentDepth === 1) {
    // User-specified selectors
    targets = await Promise.all(
      opts.targets.map(async (sel) => {
        const loc = page.locator(sel).first();
        const label = await loc.textContent().catch(() => '') ?? '';
        const tag = await loc.evaluate((el: Element) => el.tagName.toLowerCase()).catch(() => 'unknown');
        return {
          selector: sel,
          label: label.trim().slice(0, 60),
          tag,
          testId: await loc.getAttribute('data-testid').catch(() => null) ?? undefined,
        };
      }),
    );
  } else {
    targets = await discoverTargets(page);
  }

  console.log(`  Found ${targets.length} clickable targets (max: ${opts.maxClicks})`);
  const limited = targets.slice(0, opts.maxClicks);

  // Click each target
  const results: ClickResult[] = [];
  for (let i = 0; i < limited.length; i++) {
    const result = await clickTarget(page, limited[i], i + 1, ssDir, opts);
    if (result) results.push(result);
  }

  // Depth 2: follow navigation links
  if (currentDepth < opts.depth) {
    const navResults = results.filter((r) => r.type === 'navigation' && r.after.urlChanged);
    const visited = new Set<string>();

    for (const navResult of navResults) {
      const targetUrl = navResult.after.url;
      if (visited.has(targetUrl)) continue;
      visited.add(targetUrl);

      const subDir = path.join(outDir, `depth-${currentDepth + 1}-${new URL(targetUrl).pathname.replace(/\//g, '-').replace(/^-/, '') || 'root'}`);
      const subResults = await processPage(page, targetUrl, subDir, { ...opts, targets: [] }, currentDepth + 1);
      results.push(...subResults.map((r) => ({ ...r, index: results.length + r.index })));
    }
  }

  return results;
}

// ── Main ──
async function main() {
  const opts = parseArgs();

  const browser = await chromium.launch({ headless: true });
  const ctx = await browser.newContext({
    viewport: { width: opts.viewportWidth, height: opts.viewportHeight },
  });
  const page = await ctx.newPage();

  // Auth if needed
  if (opts.auth) {
    const origin = new URL(opts.url).origin;
    await login(page, origin, opts);
  }

  const results = await processPage(page, opts.url, opts.output, opts, 1);

  // Classify results
  const navCount = results.filter((r) => r.type === 'navigation').length;
  const expandCount = results.filter((r) => r.type === 'expand').length;
  const otherCount = results.filter((r) => r.type !== 'navigation' && r.type !== 'expand').length;

  // Save results
  const report = {
    url: opts.url,
    timestamp: new Date().toISOString(),
    totalClicks: results.length,
    summary: {
      navigation: navCount,
      expand: expandCount,
      other: otherCount,
    },
    clicks: results,
  };

  fs.writeFileSync(path.join(opts.output, 'click-results.json'), JSON.stringify(report, null, 2));

  // Meta
  const meta = {
    url: opts.url,
    timestamp: new Date().toISOString(),
    viewport: { width: opts.viewportWidth, height: opts.viewportHeight },
    depth: opts.depth,
    maxClicks: opts.maxClicks,
    targetsProvided: opts.targets.length > 0,
    totalClicks: results.length,
  };
  fs.writeFileSync(path.join(opts.output, 'click-meta.json'), JSON.stringify(meta, null, 2));

  // Summary
  console.log(`\n${'═'.repeat(50)}`);
  console.log(`  Click Crawl Complete`);
  console.log(`${'═'.repeat(50)}`);
  console.log(`  URL:         ${opts.url}`);
  console.log(`  Clicks:      ${results.length}`);
  console.log(`  Navigation:  ${navCount}`);
  console.log(`  Expand:      ${expandCount}`);
  console.log(`  Other:       ${otherCount}`);
  console.log(`  Output:      ${opts.output}/`);
  console.log(`${'═'.repeat(50)}`);

  await browser.close();
}

main().catch((err) => {
  console.error('❌ Error:', err.message);
  process.exit(1);
});
