import { test } from 'node:test';
import assert from 'node:assert/strict';
import { assemblePageText, batchPages, snippet } from '../src/text.js';

test('assembles items, collapses whitespace, keeps accents', () => {
  const t = assemblePageText([
    { str: 'Las autoridades', hasEOL: false },
    { str: ' tenían  conocimiento', hasEOL: true },
    { str: 'previo del riesgo.', hasEOL: true },
  ]);
  assert.equal(t, 'Las autoridades tenían conocimiento previo del riesgo.');
});

test('re-joins words hyphenated at line end, keeps real hyphens', () => {
  assert.equal(assemblePageText([{ str: 'conoci-', hasEOL: true }, { str: 'miento previo' }]), 'conocimiento previo');
  assert.equal(assemblePageText([{ str: 'Sentencia T-', hasEOL: true }, { str: '123' }]), 'Sentencia T- 123');
  assert.equal(assemblePageText([{ str: 'ex-ministro' }]), 'ex-ministro');
});

test('empty page', () => {
  assert.equal(assemblePageText([]), '');
  assert.equal(assemblePageText([{ str: '   ', hasEOL: true }]), '');
});

test('batchPages keeps order and never splits a page', () => {
  const pages = [1, 2, 3, 4, 5].map((page) => ({ page, text: 'x'.repeat(page * 10) }));
  const b = batchPages(pages, 35);
  assert.deepEqual(b.flat().map((p) => p.page), [1, 2, 3, 4, 5]);
  for (const batch of b) assert.ok(batch.length === 1 || batch.reduce((n, p) => n + p.text.length, 0) <= 35);
  assert.deepEqual(batchPages([], 10), []);
});

test('snippet trims at a word boundary', () => {
  assert.equal(snippet('corto'), 'corto');
  const s = snippet('palabra '.repeat(100), 50);
  assert.ok(s.endsWith('…') && s.length <= 51);
});
