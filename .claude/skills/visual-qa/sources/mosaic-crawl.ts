/**
 * Mosaic Crawl — 페이지를 타일 단위로 분할 촬영
 *
 * Usage:
 *   npx tsx src/mosaic-crawl.ts <url> [options]
 *
 * Options:
 *   --output=DIR          결과 디렉토리 (default: ./output-mosaic)
 *   --tile-height=N       타일 높이 px (default: 720)
 *   --viewport-width=N    뷰포트 너비 (default: 1440)
 *   --viewport-height=N   뷰포트 높이 (default: 900)
 *   --wait=N              페이지 로드 후 대기 ms (default: 4000)
 *   --scroll-wait=N       스크롤 후 대기 ms (default: 400)
 *   --auth                로그인 필요 시
 *   --auth-email=STR      로그인 이메일
 *   --auth-password=STR   로그인 비밀번호
 *   --login-url=STR       로그인 페이지 URL
 *   --pages=URL1,URL2     여러 페이지 순회
 */
import { chromium, type Page } from 'playwright';
import * as fs from 'fs';
import * as path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

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
    console.error('Usage: npx tsx src/mosaic-crawl.ts <url> [options]');
    process.exit(1);
  }

  return {
    url,
    output: path.resolve(opts['output'] ?? './output-mosaic'),
    tileHeight: parseInt(opts['tile-height'] ?? '720'),
    viewportWidth: parseInt(opts['viewport-width'] ?? '1440'),
    viewportHeight: parseInt(opts['viewport-height'] ?? '900'),
    wait: parseInt(opts['wait'] ?? '4000'),
    scrollWait: parseInt(opts['scroll-wait'] ?? '400'),
    auth: opts['auth'] === 'true',
    authEmail: opts['auth-email'] ?? '',
    authPassword: opts['auth-password'] ?? '',
    loginUrl: opts['login-url'] ?? '',
    pages: opts['pages']?.split(',').filter(Boolean) ?? [],
  };
}

// ── Login ──
async function login(page: Page, origin: string, opts: ReturnType<typeof parseArgs>) {
  const loginUrl = opts.loginUrl || `${origin}/login`;
  console.log(`🔑 Logging in at ${loginUrl}...`);
  await page.goto(loginUrl, { waitUntil: 'networkidle', timeout: 30000 });

  // Try email/password form
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

// ── DOM structure extraction ──
// Uses string-based evaluate to avoid esbuild __name injection (bug #35)
async function extractDomStructure(page: Page) {
  return page.evaluate(`(() => {
    const headings = Array.from(document.querySelectorAll('h1,h2,h3,h4,h5,h6')).map((el) => ({
      tag: el.tagName.toLowerCase(),
      text: (el.textContent || '').trim().slice(0, 100),
    }));

    const sections = Array.from(document.querySelectorAll('section, [data-testid]')).map((el) => ({
      tag: el.tagName.toLowerCase(),
      testId: el.getAttribute('data-testid') || undefined,
      childCount: el.children.length,
    }));

    const navItems = Array.from(document.querySelectorAll('nav a, nav button')).map((el) => ({
      tag: el.tagName.toLowerCase(),
      text: (el.textContent || '').trim().slice(0, 60),
      href: el.getAttribute('href') || undefined,
    }));

    return { headings, sections, navItems };
  })()`) as any;
}

// ── Mosaic capture for one page ──
async function captureMosaic(
  page: Page,
  url: string,
  outDir: string,
  opts: ReturnType<typeof parseArgs>,
) {
  const ssDir = path.join(outDir, 'screenshots');
  fs.mkdirSync(ssDir, { recursive: true });

  console.log(`\n📸 Capturing: ${url}`);
  await page.goto(url, { waitUntil: 'networkidle', timeout: 60000 });
  await page.waitForTimeout(opts.wait);

  // Full page screenshot
  await page.screenshot({ path: path.join(ssDir, '00-full-page.png'), fullPage: true });
  console.log('  ✅ Full page');

  // Page metrics — string-based evaluate to avoid esbuild __name injection (bug #35)
  const totalHeight = await page.evaluate(`document.body.scrollHeight`) as number;
  const totalWidth = await page.evaluate(`document.body.scrollWidth`) as number;
  console.log(`  Page: ${totalWidth}x${totalHeight}px`);

  // Tile capture
  const step = opts.tileHeight;
  let y = 0;
  let idx = 0;
  while (y < totalHeight) {
    await page.evaluate(`window.scrollTo(0, ${y})`);
    await page.waitForTimeout(opts.scrollWait);
    await page.screenshot({
      path: path.join(ssDir, `tile-${String(idx).padStart(2, '0')}.png`),
    });
    console.log(`  ✅ tile-${String(idx).padStart(2, '0')} at y=${y}`);
    y += step;
    idx++;
  }

  // Scroll back to top
  await page.evaluate(`window.scrollTo(0, 0)`);

  // Text extraction
  const textContent = await page.evaluate(`document.body.innerText`) as string;
  fs.writeFileSync(path.join(outDir, 'text-full.txt'), textContent);

  // DOM structure
  const domStructure = await extractDomStructure(page);
  fs.writeFileSync(path.join(outDir, 'dom-structure.json'), JSON.stringify(domStructure, null, 2));

  // Meta
  const meta = {
    url,
    timestamp: new Date().toISOString(),
    viewport: { width: opts.viewportWidth, height: opts.viewportHeight },
    pageHeight: totalHeight,
    tileHeight: step,
    tileCount: idx,
    textLength: textContent.length,
    headingCount: domStructure.headings.length,
    sectionCount: domStructure.sections.length,
  };
  fs.writeFileSync(path.join(outDir, 'mosaic-meta.json'), JSON.stringify(meta, null, 2));

  console.log(`  ✅ ${idx} tiles + text + DOM structure`);
  return meta;
}

// ── Main ──
async function main() {
  const opts = parseArgs();
  const origin = new URL(opts.url).origin;

  const browser = await chromium.launch({ headless: true });
  const ctx = await browser.newContext({
    viewport: { width: opts.viewportWidth, height: opts.viewportHeight },
  });
  const page = await ctx.newPage();

  // Auth if needed
  if (opts.auth) {
    await login(page, origin, opts);
  }

  const allPages = [opts.url, ...opts.pages];

  if (allPages.length === 1) {
    // Single page mode
    const meta = await captureMosaic(page, opts.url, opts.output, opts);
    console.log(`\n📊 Done: ${meta.tileCount} tiles saved to ${opts.output}/`);
  } else {
    // Multi-page mode
    fs.mkdirSync(opts.output, { recursive: true });
    const summaries: Array<{ page: string; dir: string; meta: any }> = [];

    for (let i = 0; i < allPages.length; i++) {
      const pageUrl = allPages[i];
      // Derive a short name from the URL path
      const urlPath = new URL(pageUrl).pathname.replace(/\//g, '-').replace(/^-/, '') || 'root';
      const pageDir = path.join(opts.output, `page-${i}-${urlPath}`);

      const meta = await captureMosaic(page, pageUrl, pageDir, opts);
      summaries.push({ page: pageUrl, dir: `page-${i}-${urlPath}`, meta });
    }

    // Write summary
    fs.writeFileSync(
      path.join(opts.output, 'summary.json'),
      JSON.stringify(
        {
          timestamp: new Date().toISOString(),
          totalPages: allPages.length,
          pages: summaries,
        },
        null,
        2,
      ),
    );
    console.log(`\n📊 Done: ${allPages.length} pages captured to ${opts.output}/`);
  }

  await browser.close();
}

main().catch((err) => {
  console.error('❌ Error:', err.message);
  process.exit(1);
});
