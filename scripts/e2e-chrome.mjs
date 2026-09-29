// Full UI end-to-end test in a real Chrome (Chrome for Testing, because branded
// Chrome >= 137 ignores --load-extension). Uses the INSTALLED engine (the
// Native Messaging registration written by LegalSearchSetup.exe).
//
//   node --experimental-websocket scripts/e2e-chrome.mjs --chrome <path\to\chrome.exe>
//
// Chrome for Testing: https://googlechromelabs.github.io/chrome-for-testing/
// (a developer tool only; nothing here ships to users). Drives Chrome through
// the DevTools protocol: opens a PDF, opens the side panel, waits for indexing,
// searches, clicks "Go to page", checks the PDF viewer page, reopens the panel
// to verify the cache, and checks the scanned-PDF message.
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, readFileSync, readdirSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const chromeArg = process.argv.indexOf('--chrome');
const chromeExe = chromeArg > 0 ? process.argv[chromeArg + 1] : null;
if (!chromeExe || !existsSync(chromeExe)) throw new Error('pass --chrome <chrome-for-testing\\chrome.exe>');
const extDir = join(root, 'dist', 'extension');
const extId = JSON.parse(readFileSync(join(root, 'config', 'extension.config.json'), 'utf8')).extensionId;
const PDF_VIEWER = 'chrome-extension://mhjfbmdgcfjbbpaeojofohoefgiehjai/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
// Optional: SCREENSHOTS_DIR=<dir> saves real screenshots of this run (used for docs/assets).
const shotsDir = process.env.SCREENSHOTS_DIR;
async function shot(session, name) {
  if (!shotsDir) return;
  const { data } = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false }, session);
  (await import('node:fs')).writeFileSync(join(shotsDir, name), Buffer.from(data, 'base64'));
  console.log(`        screenshot: ${name}`);
}

let failures = 0;
const check = (ok, what) => { console.log(`${ok ? '  PASS' : '  FAIL'}  ${what}`); if (!ok) failures++; };

// ------------------------------------------------------------- local PDF server
// fresh.pdf = fixture.pdf + a unique PDF comment: new SHA-256, so the side panel
// must extract and index it from scratch (the engine cache cannot short-circuit).
const fresh = Buffer.concat([readFileSync(join(root, 'tests', 'fixtures', 'fixture.pdf')), Buffer.from(`
% e2e ${Date.now()}
`)]);
const server = createServer((req, res) => {
  const name = decodeURIComponent(new URL(req.url, 'http://x').pathname).replace(/^\/docs\//, '');
  if (name === 'fresh.pdf') { res.writeHead(200, { 'content-type': 'application/pdf' }).end(fresh); return; }
  const file = join(root, 'tests', 'fixtures', name);
  if (!/^[a-z]+\.pdf$/.test(name) || !existsSync(file)) { res.writeHead(404).end(); return; }
  res.writeHead(200, { 'content-type': 'application/pdf' }).end(readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}/docs`;

// ------------------------------------------------------------- chrome
const profile = mkdtempSync(join(tmpdir(), 'legalsearch-chrome-'));
// --remote-debugging-pipe + --enable-unsafe-extension-debugging unlock the CDP
// Extensions domain: loadUnpacked and triggerAction (a real toolbar-button click).
const chrome = spawn(chromeExe, [
  `--user-data-dir=${profile}`, '--remote-debugging-pipe', '--enable-unsafe-extension-debugging',
  '--no-first-run', '--no-default-browser-check', '--disable-sync', '--window-size=1400,900', 'about:blank',
], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
const toChrome = chrome.stdio[3];
const fromChrome = chrome.stdio[4];

// Minimal CDP client over the pipe (NUL-delimited JSON, flattened sessions).
let seq = 0;
const pending = new Map();
let inbuf = '';
fromChrome.on('data', (d) => {
  inbuf += d.toString('utf8');
  let i;
  while ((i = inbuf.indexOf('\0')) >= 0) {
    const m = JSON.parse(inbuf.slice(0, i));
    inbuf = inbuf.slice(i + 1);
    if (m.id && pending.has(m.id)) {
      const { resolve, reject } = pending.get(m.id);
      pending.delete(m.id);
      m.error ? reject(new Error(m.error.message)) : resolve(m.result);
    }
  }
});
const send = (method, params = {}, sessionId) =>
  new Promise((resolve, reject) => {
    const id = ++seq;
    pending.set(id, { resolve, reject });
    toChrome.write(JSON.stringify({ id, method, params, sessionId }) + '\0');
  });
const { id: loadedId } = await send('Extensions.loadUnpacked', { path: extDir });
if (loadedId !== extId) throw new Error(`extension id ${loadedId} != configured ${extId}`);
const targets = async () => (await send('Target.getTargets')).targetInfos;
async function waitTarget(pred, what, ms = 20000) {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    const t = (await targets()).find(pred);
    if (t) return t;
    await sleep(150);
  }
  throw new Error(`timeout waiting for ${what}`);
}
const attach = async (t) => (await send('Target.attachToTarget', { targetId: t.targetId, flatten: true })).sessionId;
async function evaluate(session, expression, userGesture = false) {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true, userGesture }, session);
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  return r.result.value;
}

async function openSidePanel(tabTargetId) {
  // Exactly what the user does: click the LegalSearch toolbar button (needs the
  // "tab" target that owns the page).
  const page = (await targets()).find((t) => t.targetId === tabTargetId);
  const tabs = (await send('Target.getTargets', { filter: [{ type: 'tab' }] })).targetInfos;
  const tab = tabs.find((t) => t.url === page.url) ?? tabs[0];
  await send('Extensions.triggerAction', { id: extId, targetId: tab.targetId });
  const panel = await waitTarget((t) => t.url === `chrome-extension://${extId}/sidepanel.html`, 'side panel');
  const ps = await attach(panel);
  for (let i = 0; i < 100 && !(await evaluate(ps, `!!document.getElementById('status-text')`).catch(() => false)); i++) await sleep(50);
  // Record every status shown, so we can assert on the progress UI afterwards.
  await evaluate(ps, `(() => { window.__seen = [];
    const rec = () => { const t = document.getElementById('status-text').textContent + ' ' + document.getElementById('status-detail').textContent;
      if (window.__seen.at(-1) !== t) window.__seen.push(t); };
    rec(); new MutationObserver(rec).observe(document.getElementById('status'), { subtree: true, childList: true, characterData: true, attributes: true }); })()`);
  return { panel, ps };
}
async function waitStatus(ps, re, ms = 90000) {
  const t0 = Date.now();
  let last = '';
  while (Date.now() - t0 < ms) {
    last = await evaluate(ps, `document.getElementById('status-text').textContent`);
    if (re.test(last)) return last;
    await sleep(100);
  }
  throw new Error(`status timeout; last: ${last}`);
}
async function viewerPage(tabTargetId) {
  // The Chrome PDF viewer runs as an inner extension frame of the tab.
  for (let i = 0; i < 40; i++) {
    const all = (await send('Target.getTargets', { filter: [{}] })).targetInfos;
    const v = all.find((t) => t.url.startsWith(PDF_VIEWER));
    if (v) {
      const s = await attach(v);
      try {
        const n = await evaluate(s, `(() => { const v = document.querySelector('pdf-viewer'); return v && v.viewport_ ? v.viewport_.getMostVisiblePage() + 1 : null; })()`);
        if (n) return n;
      } catch { /* not ready */ }
    }
    await sleep(250);
  }
  return null;
}

try {
  console.log(`Chrome UI E2E (${chromeExe})`);
  const corpus = JSON.parse(readFileSync(join(root, 'tests', 'fixtures', 'corpus.json'), 'utf8'));
  const q = corpus.queries.find((x) => x.noSharedWords);

  // ---- 1. open PDF, open panel, first-time indexing
  const { targetId: tabId } = await send('Target.createTarget', { url: `${base}/fresh.pdf` });
  await sleep(1500);
  let { panel, ps } = await openSidePanel(tabId);
  const t0 = Date.now();
  await waitStatus(ps, /Document ready/);
  const seen = await evaluate(ps, 'window.__seen');
  check(true, `document indexed in the side panel (${Date.now() - t0} ms)`);
  console.log('        statuses: ' + seen.map((s) => s.trim()).join(' → '));
  check(seen.some((s) => /Extracting/.test(s)) && seen.some((s) => /Building semantic index/.test(s)), 'first-time progress shown (extracting → generating index)');
  check(seen.some((s) => /\d+ %/.test(s)), 'percentage shown during preparation');
  check(await evaluate(ps, `document.body.innerText.includes('Local processing. The document never leaves this computer.')`), 'privacy notice visible');

  // ---- 2. semantic search
  await evaluate(ps, `document.getElementById('query').value = ${JSON.stringify(q.q)}; document.getElementById('search-form').requestSubmit()`);
  await sleep(200);
  for (let i = 0; i < 50 && !(await evaluate(ps, `document.querySelectorAll('.result').length`)); i++) await sleep(100);
  const results = await evaluate(ps, `[...document.querySelectorAll('.result')].map(li => ({ page: li.querySelector('.result-page').textContent, text: li.querySelector('blockquote').textContent }))`);
  check(results.length > 0, `"${q.q}" → ${results.length} results`);
  results.slice(0, 3).forEach((r) => console.log(`        ${r.page}: ${r.text.slice(0, 90)}…`));
  const idx = results.findIndex((r) => r.page === `Page ${q.page}`);
  check(idx >= 0 && idx < 3, `target page ${q.page} in top-3 with the target text`);
  check(results[idx]?.text.includes('Las autoridades tenían conocimiento previo'), 'fragment text shown');
  await shot(ps, 'side-panel-results.png');

  // ---- 2b. optional local feedback + export
  const exportLabel = () => evaluate(ps, `document.getElementById('export-btn').textContent`);
  const other0 = results.findIndex((r) => r.page !== `Page ${q.page}`);
  const clickFb = (i, cls) => evaluate(ps, `document.querySelectorAll('.result')[${i}].querySelector('.feedback button${cls}').click()`);
  await clickFb(idx, ':not(.neg)'); // Relevant
  await clickFb(other0, '.neg'); // Not relevant
  await sleep(300);
  await shot(ps, 'side-panel-feedback.png');
  await sleep(300);
  check(/\(2\)/.test(await exportLabel()), `feedback saved locally → "${await exportLabel()}"`);
  await clickFb(other0, '.neg'); // clicking again clears the judgement
  await sleep(300);
  check(/\(1\)/.test(await exportLabel()), 'clicking the active choice again removes it');
  await clickFb(other0, '.neg');
  await evaluate(ps, `document.getElementById('missing-open').click();
    document.getElementById('missing-page').value = '4';
    document.getElementById('missing-note').value = 'deadline to sue';
    document.getElementById('missing-form').requestSubmit()`);
  await sleep(400);
  check(/\(3\)/.test(await exportLabel()) && (await evaluate(ps, `document.getElementById('missing-done').textContent`)) === 'Saved on this computer.', 'missing-result feedback saved');

  const dlDir = mkdtempSync(join(tmpdir(), 'legalsearch-dl-'));
  await send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: dlDir, eventsEnabled: true });
  await evaluate(ps, `document.getElementById('export-btn').click()`);
  let exported = null;
  for (let i = 0; i < 50 && !exported; i++) {
    await sleep(100);
    const f = readdirSync(dlDir).find((n) => n.endsWith('.json'));
    if (f) try { exported = JSON.parse(readFileSync(join(dlDir, f), 'utf8')); exported.__file = f; } catch { exported = null; }
  }
  check(exported?.format === 'legalsearch-feedback/1' && exported.recordCount === 3, `export file ${exported?.__file} (${exported?.recordCount} records)`);
  const rel = exported?.records.find((r) => r.type === 'result' && r.judgement === 'relevant');
  const miss = exported?.records.find((r) => r.type === 'missing');
  check(
    rel && rel.query === q.q && /^[0-9a-f]{64}$/.test(rel.documentSha256) && rel.retrievalMode === 'dense' &&
      rel.selected.page === q.page && rel.results.length === results.length && rel.selected.snippet.length <= 300,
    'export has hash, query, ranked results, selected passage, judgement, mode',
  );
  check(miss?.expectedPage === 4 && miss.note === 'deadline to sue', 'export has the missing-result report');
  check(JSON.stringify(exported).length < 20000, 'export does not contain the document text');
  rmSync(dlDir, { recursive: true, force: true });
  await evaluate(ps, `window.confirm = () => true; document.getElementById('delete-btn').click()`);
  await sleep(400);
  check(/\(0\)/.test(await exportLabel()), 'Delete removes all local feedback');

  // ---- 3. "Go to page"
  await evaluate(ps, `document.querySelectorAll('.result .go')[${idx}].click()`);
  await sleep(2500);
  const tab = (await targets()).find((t) => t.targetId === tabId);
  check(tab.url === `${base}/fresh.pdf#page=${q.page}`, `tab URL → ${tab.url.replace(base, '')}`);
  await shot(await attach(tab), 'pdf-go-to-page.png');
  let vp = await viewerPage(tabId);
  check(vp === q.page, `PDF viewer shows page ${vp}`);
  // Jump elsewhere, then back to the same page (URL would be unchanged).
  const other = results.findIndex((r) => r.page !== `Page ${q.page}`);
  await evaluate(ps, `document.querySelectorAll('.result .go')[${other}].click()`);
  await sleep(2500);
  const vpOther = await viewerPage(tabId);
  check(vpOther === results[other].page.match(/\d+/).map(Number)[0], `jump to ${results[other].page} (viewer page ${vpOther})`);
  await evaluate(ps, `document.querySelectorAll('.result .go')[${idx}].click()`);
  await sleep(2500);
  vp = await viewerPage(tabId);
  check(vp === q.page, `back to page ${q.page} after jumping to ${results[other].page} (viewer page ${vp})`);
  check(await evaluate(ps, `document.querySelectorAll('.result').length`) === results.length, 'results kept after navigation (no re-indexing)');

  // ---- 4. close panel (engine process ends), reopen: cache hit
  await send('Target.closeTarget', { targetId: panel.targetId });
  await sleep(1500);
  ({ panel, ps } = await openSidePanel(tabId));
  const t1 = Date.now();
  await waitStatus(ps, /Document ready/);
  const seen2 = await evaluate(ps, 'window.__seen');
  check(!seen2.some((s) => /Extracting|Building/.test(s)), `reopened from cache in ${Date.now() - t1} ms (no extraction/indexing)`);

  // ---- 5. scanned PDF
  await send('Page.navigate', { url: `${base}/scanned.pdf` }, await attach(tab));
  const msg = await waitStatus(ps, /no extractable text/);
  const detail = await evaluate(ps, `document.getElementById('status-detail').textContent`);
  check(/scanned/.test(detail) && /OCR/.test(detail), `scanned PDF → "${msg}" / "${detail.slice(0, 60)}…"`);

  // ---- 6. local PDF (file://). Extensions loaded by DevTools get file access
  // by default, so this exercises the real file:// read + navigation path. (The
  // "permission needed" message for users who did not enable file access is a
  // manual check, see docs/installation.md.)
  const fileUrl = 'file:///' + join(root, 'tests', 'fixtures', 'fixture.pdf').split(String.fromCharCode(92)).join('/');
  await send('Page.navigate', { url: fileUrl }, await attach(tab));
  await waitStatus(ps, /Document ready|permission/, 30000);
  const fstatus = await evaluate(ps, `document.getElementById('status-text').textContent`);
  if (/permission/.test(fstatus)) {
    check(true, `file:// without file access → "${fstatus}"`);
  } else {
    await evaluate(ps, `document.getElementById('query').value = ${JSON.stringify(corpus.queries[2].q)}; document.getElementById('search-form').requestSubmit()`);
    await sleep(800);
    const fr = await evaluate(ps, `[...document.querySelectorAll('.result .result-page')].map(e => e.textContent)`);
    const want = `Page ${corpus.queries[2].page}`;
    check(fr[0] === want, `file:// PDF searchable ("${corpus.queries[2].q}" → ${fr[0]})`);
    await evaluate(ps, `document.querySelectorAll('.result .go')[0].click()`);
    await sleep(2500);
    const fu = (await targets()).find((t) => t.targetId === tabId).url;
    const fv = await viewerPage(tabId);
    check(fu === `${fileUrl}#page=${corpus.queries[2].page}` && fv === corpus.queries[2].page, `file:// "Go to page" → ${fu.split('/').pop()}, viewer page ${fv}`);
  }

  // ---- 7. a non-PDF page
  await send('Page.navigate', { url: 'about:blank' }, await attach(tab));
  await sleep(800);
  const np = await evaluate(ps, `document.getElementById('status-text').textContent`);
  check(/cannot be analyzed|Open a PDF/.test(np), `non-PDF tab → "${np}"`);
} catch (e) {
  console.log('  FAIL  ' + e.message);
  failures++;
} finally {
  try { await Promise.race([send('Browser.close'), sleep(3000)]); } catch {}
  chrome.kill();
  server.close();
  await sleep(800);
  rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
}
console.log(failures ? `\n${failures} check(s) FAILED` : '\nALL CHECKS PASSED');
process.exit(failures ? 1 : 0);
