// End-to-end test of LegalSearch.exe over the real Native Messaging protocol,
// exactly as Chrome drives it (stdio, 4-byte little-endian length prefix,
// origin argument). Measures indexing time, search latency and RAM.
//
//   node scripts/e2e-host.mjs --dev         target/release exe + repo model (temp cache)
//   node scripts/e2e-host.mjs --installed   %LOCALAPPDATA%\LegalSearch\bin\LegalSearch.exe
//
// Prerequisite: tests/fixtures/*.pages.json (produced by `npm test` in extension/,
// which runs the real PDF.js extraction on the fixture PDFs).
import { spawn, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const fx = (f) => join(root, 'tests', 'fixtures', f);
const config = JSON.parse(readFileSync(join(root, 'config', 'extension.config.json'), 'utf8'));
const ORIGIN = `chrome-extension://${config.extensionId}/`;
const installed = process.argv.includes('--installed');

let exe, env = { ...process.env }, tempData = null;
if (installed) {
  exe = join(process.env.LOCALAPPDATA, 'LegalSearch', 'bin', 'LegalSearch.exe');
} else {
  exe = join(root, 'native', 'target', 'release', 'LegalSearch.exe');
  tempData = mkdtempSync(join(tmpdir(), 'legalsearch-e2e-'));
  env.LEGALSEARCH_DATA_DIR = tempData;
  env.LEGALSEARCH_MODEL_DIR = join(root, 'models', 'current');
  env.LEGALSEARCH_ORT_DLL = join(root, 'native', 'runtime', 'onnxruntime.dll');
}
if (!existsSync(exe)) throw new Error(`not found: ${exe}`);

let failures = 0;
const check = (ok, what) => {
  console.log(`${ok ? '  PASS' : '  FAIL'}  ${what}`);
  if (!ok) failures++;
};

function memMB(pid) {
  try {
    const out = execFileSync('powershell', ['-NoProfile', '-Command',
      `$p = Get-Process -Id ${pid}; "$($p.WorkingSet64) $($p.PeakWorkingSet64) $($p.PrivateMemorySize64)"`], { encoding: 'utf8' });
    const [ws, peak, priv] = out.trim().split(/\s+/).map((n) => Number(n) / 1048576);
    return { ws, peak, priv };
  } catch {
    return { ws: NaN, peak: NaN, priv: NaN };
  }
}

class Host {
  constructor() {
    this.proc = spawn(exe, [ORIGIN, '--parent-window=0'], { env, stdio: ['pipe', 'pipe', 'inherit'], windowsHide: true });
    this.buf = Buffer.alloc(0);
    this.waiters = [];
    this.events = [];
    this.id = 1;
    this.exited = new Promise((r) => this.proc.on('exit', (code) => r(code)));
    this.proc.stdout.on('data', (d) => {
      this.buf = Buffer.concat([this.buf, d]);
      while (this.buf.length >= 4) {
        const n = this.buf.readUInt32LE(0);
        if (this.buf.length < 4 + n) break;
        const msg = JSON.parse(this.buf.subarray(4, 4 + n).toString('utf8'));
        this.buf = this.buf.subarray(4 + n);
        this.events.push(msg);
        this.waiters = this.waiters.filter((w) => !w(msg));
      }
    });
  }
  sendRaw(bytes) {
    const h = Buffer.alloc(4);
    h.writeUInt32LE(bytes.length);
    this.proc.stdin.write(Buffer.concat([h, bytes]));
  }
  /** Sends a request; resolves with its final response. `onProgress` gets progress events. */
  req(body, onProgress) {
    const id = this.id++;
    return new Promise((resolve) => {
      this.waiters.push((m) => {
        if (m.id !== id && !(m.id === 0 && m.type === 'error')) return false;
        if (m.type === 'progress') { onProgress?.(m); return false; }
        resolve(m);
        return true;
      });
      this.sendRaw(Buffer.from(JSON.stringify({ ...body, id })));
    });
  }
  async close() {
    this.proc.stdin.end();
    return this.exited;
  }
}

const sha = (f) => createHash('sha256').update(readFileSync(f)).digest('hex');
const pagesOf = (f) => JSON.parse(readFileSync(fx(f), 'utf8'));
const corpus = JSON.parse(readFileSync(fx('corpus.json'), 'utf8'));

async function index(h, sha256, pages, label) {
  let lastPct = -1, peak = 0;
  const poll = setInterval(() => { peak = Math.max(peak, memMB(h.proc.pid).ws || 0); }, 250);
  const t = performance.now();
  await h.req({ type: 'index_begin', sha256, filename: label, pageCount: pages.length });
  for (let i = 0; i < pages.length; i += 50) await h.req({ type: 'index_pages', sha256, pages: pages.slice(i, i + 50) });
  const r = await h.req({ type: 'index_end', sha256 }, (p) => { lastPct = p.pct; });
  const ms = performance.now() - t;
  clearInterval(poll);
  const m = memMB(h.proc.pid);
  return { r, ms, lastPct, peakWs: Math.max(peak, m.peak) };
}

console.log(`LegalSearch E2E (${installed ? 'installed' : 'dev'}): ${exe}`);
const report = {};
try {
  // ---------------------------------------------------------------- session 1
  console.log('\n[1] first open: cache miss -> index -> search');
  let h = new Host();
  const hello = await h.req({ type: 'hello' });
  check(hello.type === 'hello' && hello.protocol === 1, `hello (engine v${hello.version})`);
  await new Promise((r) => setTimeout(r, 300));
  report.idleMB = memMB(h.proc.pid).ws;

  const fsha = sha(fx('fixture.pdf'));
  const pages = pagesOf('fixture.pages.json');
  let o = await h.req({ type: 'open', sha256: fsha, filename: 'fixture.pdf', pageCount: pages.length });
  check(o.type === 'opened' && o.status === 'needs_index', 'open -> needs_index (cache miss)');

  const ix = await index(h, fsha, pages, 'fixture.pdf');
  check(ix.r.type === 'ready' && ix.r.chunkCount > 0, `indexed ${pages.length} pages -> ${ix.r.chunkCount} chunks in ${ix.ms.toFixed(0)} ms`);
  check(ix.lastPct === 100, 'progress reached 100 %');
  report.indexMs = ix.ms;
  report.indexPeakMB = ix.peakWs;

  let hit1 = 0, hit3 = 0, lat = [];
  for (const q of corpus.queries) {
    const t = performance.now();
    const r = await h.req({ type: 'search', sha256: fsha, query: q.q, k: 8 });
    lat.push(performance.now() - t);
    const got = r.results.map((x) => x.page);
    if (got[0] === q.page) hit1++;
    if (got.slice(0, 3).includes(q.page)) hit3++;
    if (q.noSharedWords) {
      check(got.slice(0, 3).includes(q.page), `"${q.q}" -> page ${q.page} in top-3 (got ${got.slice(0, 3)})`);
      check(r.results.some((x) => x.text.includes('Las autoridades tenían conocimiento previo')), 'target fragment text returned');
      // Dense-only: sorted by cosine. Hybrid: sorted by fused rank while the exposed
      // score stays the cosine, so it need not be monotonic.
      if (process.env.LEGALSEARCH_HYBRID !== '1') check(r.results.every((x, i, a) => i === 0 || a[i - 1].score >= x.score), 'results sorted by score');
    }
  }
  // Warm latency: repeat each query.
  const warm = [];
  for (let i = 0; i < 3; i++) for (const q of corpus.queries) {
    const t = performance.now();
    await h.req({ type: 'search', sha256: fsha, query: q.q, k: 8 });
    warm.push(performance.now() - t);
  }
  report.searchAvgMs = warm.reduce((a, b) => a + b, 0) / warm.length;
  report.searchMB = memMB(h.proc.pid).ws;
  check(hit3 / corpus.queries.length >= 0.8, `recall@1 ${hit1}/${corpus.queries.length}, recall@3 ${hit3}/${corpus.queries.length}`);
  console.log(`        search latency (round trip) avg ${report.searchAvgMs.toFixed(1)} ms`);

  // ---- scanned / empty
  const spages = pagesOf('scanned.pages.json');
  const sx = await index(h, sha(fx('scanned.pdf')), spages, 'scanned.pdf');
  check(sx.r.type === 'error' && sx.r.code === 'no_text', 'scanned PDF -> error no_text');

  // ---- invalid inputs: the host must answer with errors and keep running
  const bad = [
    [{ type: 'search', sha256: '../../etc/passwd', query: 'x' }, 'path-like sha256'],
    [{ type: 'search', sha256: fsha, query: '' }, 'empty query'],
    [{ type: 'search', sha256: fsha, query: 'x'.repeat(5000) }, 'oversized query'],
    [{ type: 'open', sha256: fsha, filename: 'a', pageCount: 0 }, 'zero pages'],
    [{ type: 'run', cmd: 'calc.exe' }, 'unknown message type'],
    [{ type: 'search', sha256: 'b'.repeat(64), query: 'x' }, 'search on unknown document'],
  ];
  for (const [body, what] of bad) {
    const r = await h.req(body);
    check(r.type === 'error', `rejects ${what} (${r.code})`);
  }
  h.sendRaw(Buffer.from('{not json'));
  await new Promise((r) => setTimeout(r, 200));
  check(h.events.at(-1)?.type === 'error', 'rejects malformed JSON');
  const alive = await h.req({ type: 'hello' });
  check(alive.type === 'hello', 'host still responsive after bad input');
  const code = await h.close();
  check(code === 0, 'exits cleanly when Chrome closes the port (stdin EOF)');

  // ---------------------------------------------------------------- session 2
  console.log('\n[2] reopen same PDF in a new process: cache hit');
  h = new Host();
  const t2 = performance.now();
  o = await h.req({ type: 'open', sha256: fsha, filename: 'fixture.pdf', pageCount: pages.length });
  const openMs = performance.now() - t2;
  check(o.status === 'ready' && o.chunkCount === ix.r.chunkCount, `open -> ready from cache in ${openMs.toFixed(0)} ms (no re-indexing)`);
  const r2 = await h.req({ type: 'search', sha256: fsha, query: corpus.queries[0].q, k: 5 });
  check(r2.results[0]?.page === corpus.queries[0].page, 'search works immediately from cache');
  report.cacheOpenMs = openMs;
  await h.close();

  // ---------------------------------------------------------------- oversize frame
  console.log('\n[3] oversized frame is refused and the process stops');
  h = new Host();
  const hdr = Buffer.alloc(4);
  hdr.writeUInt32LE(100 * 1024 * 1024);
  h.proc.stdin.write(hdr);
  const c3 = await h.exited;
  check(c3 !== 0, `refused 100 MB frame (exit ${c3})`);

  // ---------------------------------------------------------------- bench
  if (existsSync(fx('bench.pages.json'))) {
    console.log('\n[4] benchmark: 300-page document');
    h = new Host();
    const bpages = pagesOf('bench.pages.json');
    const bsha = sha(fx('bench.pdf'));
    const bo = await h.req({ type: 'open', sha256: bsha, filename: 'bench.pdf', pageCount: bpages.length });
    if (bo.status === 'needs_index') {
      const bx = await index(h, bsha, bpages, 'bench.pdf');
      check(bx.r.type === 'ready', `300 pages -> ${bx.r.chunkCount} chunks in ${(bx.ms / 1000).toFixed(1)} s, peak RAM ${bx.peakWs.toFixed(0)} MB`);
      report.benchIndexMs = bx.ms;
      report.benchChunks = bx.r.chunkCount;
      report.benchPeakMB = bx.peakWs;
    } else {
      console.log('        (already cached; skipped)');
    }
    const bl = [];
    for (const q of corpus.queries) {
      const t = performance.now();
      await h.req({ type: 'search', sha256: bsha, query: q.q, k: 8 });
      bl.push(performance.now() - t);
    }
    report.benchSearchAvgMs = bl.slice(1).reduce((a, b) => a + b, 0) / (bl.length - 1);
    console.log(`        search latency on 300 pages avg ${report.benchSearchAvgMs.toFixed(1)} ms`);
    await h.close();
  }
} finally {
  const log = join(tempData ?? join(process.env.LOCALAPPDATA, 'LegalSearch'), 'logs', 'legalsearch.log');
  if (existsSync(log)) {
    const lines = readFileSync(log, 'utf8').split(/\r?\n/).filter((l) => l.includes('bm25 index'));
    for (const l of lines.slice(-2)) console.log('        log: ' + l.replace(/^\d+ /, ''));
  }
  if (tempData) rmSync(tempData, { recursive: true, force: true });
}

console.log('\nREPORT', JSON.stringify(Object.fromEntries(Object.entries(report).map(([k, v]) => [k, Math.round(v * 10) / 10]))));
console.log(failures ? `\n${failures} check(s) FAILED` : '\nALL CHECKS PASSED');
process.exit(failures ? 1 : 0);
