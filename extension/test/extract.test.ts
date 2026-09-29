// Runs the real PDF.js text extraction (same options + assemblePageText as the
// side panel) on the generated fixtures, and writes the per-page JSON that the
// host end-to-end script (scripts/e2e-host.mjs) feeds to LegalSearch.exe.
// Node 20 lacks Promise.withResolvers (Chrome >= 119 has it); polyfill for the test only.
(Promise as any).withResolvers ??= function () {
  let resolve: any, reject: any;
  const promise = new Promise((a, b) => { resolve = a; reject = b; });
  return { promise, resolve, reject };
};
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import * as pdfjs from 'pdfjs-dist/legacy/build/pdf.mjs';
import { assemblePageText } from '../src/text.js';

const dir = join(process.cwd(), '..', 'tests', 'fixtures');

async function extract(file: string) {
  const task = pdfjs.getDocument({
    data: new Uint8Array(readFileSync(join(dir, file))),
    enableXfa: false, disableFontFace: true, useSystemFonts: false, stopAtErrors: false, verbosity: 0,
  });
  const doc = await task.promise;
  const pages: { page: number; text: string }[] = [];
  for (let n = 1; n <= doc.numPages; n++) {
    const p = await doc.getPage(n);
    const c = await p.getTextContent();
    pages.push({ page: n, text: assemblePageText(c.items as { str: string; hasEOL?: boolean }[]) });
  }
  await task.destroy();
  return pages;
}

test('fixture.pdf: 12 pages, text preserved per page, accents intact', async (t) => {
  if (!existsSync(join(dir, 'fixture.pdf'))) return t.skip('run node scripts/make-fixture.mjs');
  const corpus = JSON.parse(readFileSync(join(dir, 'corpus.json'), 'utf8'));
  const pages = await extract('fixture.pdf');
  assert.equal(pages.length, corpus.pages.length);
  pages.forEach((p, i) => assert.equal(p.text, corpus.pages[i].replace(/\s+/g, ' ').trim(), `page ${i + 1}`));
  assert.ok(pages[2].text.includes('Las autoridades tenían conocimiento previo de la situación de riesgo'));
  writeFileSync(join(dir, 'fixture.pages.json'), JSON.stringify(pages));
  for (const f of ['bench.pdf']) {
    if (existsSync(join(dir, f))) writeFileSync(join(dir, f.replace('.pdf', '.pages.json')), JSON.stringify(await extract(f)));
  }
});

test('scanned.pdf: pages exist but carry no text', async (t) => {
  if (!existsSync(join(dir, 'scanned.pdf'))) return t.skip('run node scripts/make-fixture.mjs');
  const pages = await extract('scanned.pdf');
  assert.equal(pages.length, 3);
  assert.ok(pages.every((p) => p.text === ''));
  writeFileSync(join(dir, 'scanned.pages.json'), JSON.stringify(pages));
});
