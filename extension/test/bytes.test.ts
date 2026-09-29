import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { hasPdfHeader, sha256Hex } from '../src/bytes.js';
import { S } from '../src/strings.js';

const fixture = join(process.cwd(), '..', 'tests', 'fixtures', 'fixture.pdf');

test('sha256Hex matches known vectors', async () => {
  assert.equal(await sha256Hex(new Uint8Array()), 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855');
  assert.equal(await sha256Hex(new TextEncoder().encode('abc')), 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad');
});

test('document hash of the fixture PDF is stable and equals the reference implementation', async (t) => {
  if (!existsSync(fixture)) return t.skip('run node scripts/make-fixture.mjs');
  const bytes = readFileSync(fixture);
  const web = await sha256Hex(new Uint8Array(bytes));
  assert.equal(web, createHash('sha256').update(bytes).digest('hex'));
  assert.match(web, /^[0-9a-f]{64}$/);
});

test('PDF header detection', () => {
  const enc = (s: string) => new TextEncoder().encode(s);
  assert.ok(hasPdfHeader(enc('%PDF-1.7\n')));
  assert.ok(hasPdfHeader(enc('\xef\xbb\xbfjunk %PDF-1.4')));
  assert.ok(!hasPdfHeader(enc('<html>')));
  assert.ok(!hasPdfHeader(new Uint8Array()));
});

test('user-facing strings contain no technical jargon', () => {
  const all = Object.values(S).map((v) => (typeof v === 'function' ? v(1) : v)).join(' ').toLowerCase();
  for (const w of ['embedding', 'vector', 'onnx', 'e5', 'model', 'sqlite', 'native messaging', 'stack', 'bm25']) {
    assert.ok(!all.includes(w), `jargon in UI: ${w}`);
  }
  assert.ok(all.includes('local processing. the document never leaves this computer.'));
});
