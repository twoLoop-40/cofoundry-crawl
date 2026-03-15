/**
 * credential-prompt.ts — General-purpose credential discovery + Playwright UI prompt.
 *
 * Usage:
 *   import { resolveCredentials } from './credential-prompt';
 *   const { email, password } = await resolveCredentials({ projectRoot: '.', targetUrl });
 *
 * Resolution order:
 *   1. CLI args (--auth-email, --auth-password)
 *   2. .env file pattern scan (EMAIL/PASSWORD key pairs)
 *   3. E2E test config files
 *   4. Playwright headed UI prompt (opens a browser window)
 */

import { chromium } from 'playwright';
import * as fs from 'fs';
import * as path from 'path';

export interface Credentials {
  email: string;
  password: string;
}

interface ResolveOptions {
  /** Project root directory to scan for .env (default: cwd) */
  projectRoot?: string;
  /** Target URL (shown in the prompt UI) */
  targetUrl?: string;
  /** Skip UI prompt even if no credentials found */
  noPrompt?: boolean;
}

// ── Pattern matching ──────────────────────────────────────────────────

const EMAIL_PATTERNS = [
  /email/i, /username/i, /user(?!.*(?:agent|data|dir|home|path|pool))/i,
  /login(?!.*url)/i, /^id$/i, /_id$/i,
];

const PASSWORD_PATTERNS = [
  /password/i, /passwd/i, /^pw$/i, /_pw$/i,
  /secret(?!.*(?:key|arn|manager|jwt|api|token))/i,
];

const SKIP_PATTERNS = [
  /uuid/i, /api_id/i, /client_id/i, /project_id/i, /session_id/i,
  /jwt_secret/i, /api_secret/i, /secret_key/i, /encryption/i,
  /bucket/i, /region/i, /endpoint/i, /base_url/i, /host/i, /port/i,
];

function isEmailKey(key: string): boolean {
  if (SKIP_PATTERNS.some(p => p.test(key))) return false;
  return EMAIL_PATTERNS.some(p => p.test(key));
}

function isPasswordKey(key: string): boolean {
  if (SKIP_PATTERNS.some(p => p.test(key))) return false;
  return PASSWORD_PATTERNS.some(p => p.test(key));
}

/** Group email+password keys by shared prefix */
function pairCredentials(
  emails: [string, string][],
  passwords: [string, string][]
): { email: string; password: string; source: string }[] {
  const pairs: { email: string; password: string; source: string }[] = [];

  for (const [eKey, eVal] of emails) {
    // Find password with matching prefix
    const ePrefix = eKey.replace(/[_.-]?(email|username|user|login|id)$/i, '').toLowerCase();

    for (const [pKey, pVal] of passwords) {
      const pPrefix = pKey.replace(/[_.-]?(password|passwd|pw|secret)$/i, '').toLowerCase();

      if (ePrefix === pPrefix || ePrefix === '' || pPrefix === '') {
        pairs.push({ email: eVal, password: pVal, source: `${eKey} / ${pKey}` });
      }
    }
  }

  return pairs;
}

// ── .env scanner ──────────────────────────────────────────────────────

function scanEnvFile(envPath: string): { email: string; password: string; source: string }[] {
  if (!fs.existsSync(envPath)) return [];

  const content = fs.readFileSync(envPath, 'utf-8');
  const emails: [string, string][] = [];
  const passwords: [string, string][] = [];

  for (const line of content.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) continue;

    const eqIdx = trimmed.indexOf('=');
    if (eqIdx < 0) continue;

    const key = trimmed.slice(0, eqIdx).trim();
    const val = trimmed.slice(eqIdx + 1).trim().replace(/^["']|["']$/g, '');

    if (!val) continue;

    if (isEmailKey(key)) emails.push([key, val]);
    if (isPasswordKey(key)) passwords.push([key, val]);
  }

  return pairCredentials(emails, passwords);
}

// ── E2E config scanner ────────────────────────────────────────────────

function scanE2EConfigs(projectRoot: string): { email: string; password: string; source: string }[] {
  const patterns = [
    'backend/tests/e2e/config.py',
    'tests/e2e/config.py',
    'e2e/config.ts',
    'cypress.env.json',
    'playwright/.auth/credentials.json',
  ];

  for (const relPath of patterns) {
    const fullPath = path.join(projectRoot, relPath);
    if (!fs.existsSync(fullPath)) continue;

    const content = fs.readFileSync(fullPath, 'utf-8');

    // Python: test_email = "..." / test_password = "..."
    const pyEmail = content.match(/(?:test_)?email\s*[:=]\s*["']([^"']+)["']/i);
    const pyPass = content.match(/(?:test_)?password\s*[:=]\s*["']([^"']+)["']/i);
    if (pyEmail && pyPass) {
      return [{ email: pyEmail[1], password: pyPass[1], source: relPath }];
    }

    // JSON: {"email": "...", "password": "..."}
    try {
      const json = JSON.parse(content);
      if (json.email && json.password) {
        return [{ email: json.email, password: json.password, source: relPath }];
      }
    } catch { /* not JSON */ }
  }

  return [];
}

// ── Playwright UI prompt ──────────────────────────────────────────────

const PROMPT_HTML = (targetUrl: string) => `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>Visual QA — Login Required</title>
  <style>
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body {
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
      background: #0a0f1e;
      color: #e0e6f0;
      display: flex;
      justify-content: center;
      align-items: center;
      min-height: 100vh;
    }
    .card {
      background: #111827;
      border: 1px solid #1e293b;
      border-radius: 16px;
      padding: 40px;
      width: 420px;
      box-shadow: 0 8px 32px rgba(0,0,0,0.4);
    }
    h1 { font-size: 20px; margin-bottom: 8px; }
    .subtitle { color: #94a3b8; font-size: 13px; margin-bottom: 24px; word-break: break-all; }
    label { display: block; font-size: 13px; color: #94a3b8; margin-bottom: 6px; }
    input {
      width: 100%;
      padding: 10px 14px;
      background: #0a0f1e;
      border: 1px solid #334155;
      border-radius: 8px;
      color: #e0e6f0;
      font-size: 14px;
      outline: none;
      margin-bottom: 16px;
    }
    input:focus { border-color: #3b82f6; }
    button {
      width: 100%;
      padding: 12px;
      background: #3b82f6;
      color: white;
      border: none;
      border-radius: 8px;
      font-size: 14px;
      font-weight: 600;
      cursor: pointer;
      margin-top: 8px;
    }
    button:hover { background: #2563eb; }
    .skip {
      display: block;
      text-align: center;
      margin-top: 12px;
      color: #64748b;
      font-size: 12px;
      cursor: pointer;
      text-decoration: underline;
    }
    .skip:hover { color: #94a3b8; }
    .hint {
      margin-top: 16px;
      padding: 12px;
      background: #0f172a;
      border-radius: 8px;
      font-size: 11px;
      color: #64748b;
      line-height: 1.5;
    }
  </style>
</head>
<body>
  <div class="card">
    <h1>🔐 Authentication Required</h1>
    <div class="subtitle">Target: ${targetUrl}</div>

    <form id="credForm">
      <label for="email">Email / Username</label>
      <input type="text" id="email" name="email" placeholder="user@example.com" autofocus>

      <label for="password">Password</label>
      <input type="password" id="password" name="password" placeholder="••••••••">

      <button type="submit">Continue Crawling</button>
    </form>

    <span class="skip" id="skipBtn">Skip — crawl without auth</span>

    <div class="hint">
      💡 Tip: Add credentials to your <code>.env</code> file to skip this prompt next time.<br>
      Example: <code>AUTH_EMAIL=user@example.com</code> / <code>AUTH_PASSWORD=secret</code>
    </div>
  </div>

  <script>
    document.getElementById('credForm').addEventListener('submit', (e) => {
      e.preventDefault();
      const email = document.getElementById('email').value;
      const password = document.getElementById('password').value;
      // Signal completion by setting a data attribute on body
      document.body.setAttribute('data-result', JSON.stringify({ email, password, skipped: false }));
    });
    document.getElementById('skipBtn').addEventListener('click', () => {
      document.body.setAttribute('data-result', JSON.stringify({ email: '', password: '', skipped: true }));
    });
  </script>
</body>
</html>`;

async function promptWithPlaywright(targetUrl: string): Promise<Credentials | null> {
  console.log('\n🖥️  Opening credential prompt (browser window)...');
  console.log('   Fill in your credentials and click "Continue Crawling".\n');

  const browser = await chromium.launch({ headless: false });
  const page = await browser.newPage();

  // Use a data URL to show the form
  const html = PROMPT_HTML(targetUrl);
  await page.setContent(html);

  // Wait for user to submit or skip (or close the window)
  try {
    await page.waitForFunction(
      () => document.body.hasAttribute('data-result'),
      { timeout: 300_000 } // 5 minutes max
    );

    const resultStr = await page.evaluate(() => document.body.getAttribute('data-result'));
    await browser.close();

    if (!resultStr) return null;

    const result = JSON.parse(resultStr);
    if (result.skipped) {
      console.log('⏭️  Skipped authentication.\n');
      return null;
    }

    console.log(`✅ Credentials received for: ${result.email}\n`);
    return { email: result.email, password: result.password };
  } catch {
    // User closed the browser or timeout
    try { await browser.close(); } catch { /* already closed */ }
    console.log('⏭️  Prompt closed — proceeding without auth.\n');
    return null;
  }
}

// ── Standard key names (credential-prompt가 정의하는 표준) ──────────

const STANDARD_EMAIL_KEY = 'CRAWL_AUTH_EMAIL';
const STANDARD_PASSWORD_KEY = 'CRAWL_AUTH_PASSWORD';

// ── .env writer ───────────────────────────────────────────────────────

function saveToEnv(envPath: string, email: string, password: string): void {
  let content = '';
  if (fs.existsSync(envPath)) {
    content = fs.readFileSync(envPath, 'utf-8');
  }

  const lines = content.split('\n');
  let emailWritten = false;
  let passwordWritten = false;

  // Update existing keys if present
  const updated = lines.map(line => {
    const trimmed = line.trim();
    if (trimmed.startsWith(`${STANDARD_EMAIL_KEY}=`)) {
      emailWritten = true;
      return `${STANDARD_EMAIL_KEY}=${email}`;
    }
    if (trimmed.startsWith(`${STANDARD_PASSWORD_KEY}=`)) {
      passwordWritten = true;
      return `${STANDARD_PASSWORD_KEY}=${password}`;
    }
    return line;
  });

  // Append if not already present
  if (!emailWritten || !passwordWritten) {
    // Ensure trailing newline before appending
    if (updated.length > 0 && updated[updated.length - 1] !== '') {
      updated.push('');
    }
    if (!emailWritten) {
      updated.push(`# Visual QA crawler auth (auto-saved by credential-prompt)`);
      updated.push(`${STANDARD_EMAIL_KEY}=${email}`);
    }
    if (!passwordWritten) {
      updated.push(`${STANDARD_PASSWORD_KEY}=${password}`);
    }
  }

  fs.writeFileSync(envPath, updated.join('\n'));
  console.log(`💾 Credentials saved to .env as ${STANDARD_EMAIL_KEY}/${STANDARD_PASSWORD_KEY}`);
}

// ── Main resolver ─────────────────────────────────────────────────────

export async function resolveCredentials(opts: ResolveOptions = {}): Promise<Credentials | null> {
  const projectRoot = opts.projectRoot || process.cwd();
  const targetUrl = opts.targetUrl || 'unknown';
  const envPath = path.join(projectRoot, '.env');

  // Step 1: Scan .env for standard keys first, then patterns
  const envPairs = scanEnvFile(envPath);

  // Prioritize standard keys (CRAWL_AUTH_EMAIL/PASSWORD)
  const standardPair = envPairs.find(p =>
    p.source.includes(STANDARD_EMAIL_KEY) || p.source.includes(STANDARD_PASSWORD_KEY)
  );

  if (standardPair) {
    console.log(`🔑 Found credentials: ${standardPair.email} (from ${standardPair.source})`);
    return { email: standardPair.email, password: standardPair.password };
  }

  // Step 2: Scan E2E configs
  const e2ePairs = scanE2EConfigs(projectRoot);
  const allPairs = [...envPairs, ...e2ePairs];

  if (allPairs.length === 1) {
    const p = allPairs[0];
    console.log(`🔑 Found credentials: ${p.email} (from ${p.source})`);
    return { email: p.email, password: p.password };
  }

  if (allPairs.length > 1) {
    console.log(`🔑 Found ${allPairs.length} credential pairs:`);
    allPairs.forEach((p, i) => console.log(`   ${i + 1}. ${p.email} (from ${p.source})`));
    console.log(`   → Using #1: ${allPairs[0].email}`);
    return { email: allPairs[0].email, password: allPairs[0].password };
  }

  // Step 3: No credentials found → open Playwright UI prompt
  if (opts.noPrompt) {
    console.log('⚠️  No credentials found and --no-prompt specified.');
    return null;
  }

  console.log('⚠️  No credentials found in .env or test configs.');
  const creds = await promptWithPlaywright(targetUrl);

  return creds;
}

/**
 * Save credentials to .env with standard keys.
 * Call this AFTER login succeeds — not before.
 */
export function saveCredentials(projectRoot: string, email: string, password: string): void {
  const envPath = path.join(projectRoot, '.env');
  saveToEnv(envPath, email, password);
}
