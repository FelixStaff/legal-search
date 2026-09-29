import { test } from 'node:test';
import assert from 'node:assert/strict';
import { displayName, isSupportedScheme, looksLikePdfUrl, withPage, withoutFragment } from '../src/url.js';

test('withPage replaces only the fragment (https)', () => {
  assert.equal(withPage('https://ex.com/a/b.pdf', 71), 'https://ex.com/a/b.pdf#page=71');
  assert.equal(withPage('https://ex.com/b.pdf?x=1&y=2#page=3&zoom=80', 5), 'https://ex.com/b.pdf?x=1&y=2#page=5');
  assert.equal(withPage('https://ex.com/b.pdf#', 2), 'https://ex.com/b.pdf#page=2');
});

test('withPage preserves file:// URLs byte for byte (spaces, accents, drive letters)', () => {
  const u = 'file:///C:/Users/Ana/Documents/Sentencia%20T-123%20de%202020%20(versi%C3%B3n).pdf';
  assert.equal(withPage(u, 9), u + '#page=9');
  assert.equal(withPage(u + '#page=2', 9), u + '#page=9');
});

test('withPage rejects invalid page numbers', () => {
  for (const p of [0, -1, 1.5, Number.NaN]) assert.throws(() => withPage('https://x/y.pdf', p), RangeError);
});

test('withoutFragment identifies the same document', () => {
  assert.equal(withoutFragment('https://ex.com/b.pdf#page=4'), 'https://ex.com/b.pdf');
  assert.equal(withoutFragment('https://ex.com/b.pdf'), 'https://ex.com/b.pdf');
});

test('PDF URL detection', () => {
  assert.ok(looksLikePdfUrl('https://ex.com/Doc.PDF'));
  assert.ok(looksLikePdfUrl('https://ex.com/doc.pdf?download=1#page=2'));
  assert.ok(looksLikePdfUrl('file:///C:/a%20b/c.pdf'));
  assert.ok(!looksLikePdfUrl('https://ex.com/doc.pdf.html'));
  assert.ok(!looksLikePdfUrl('chrome://extensions/'));
  assert.ok(!looksLikePdfUrl('not a url'));
  assert.ok(isSupportedScheme('file:///C:/x.pdf'));
  assert.ok(!isSupportedScheme('chrome-extension://abc/x.pdf'));
  assert.ok(!isSupportedScheme('javascript:alert(1)'));
});

test('displayName', () => {
  assert.equal(displayName('file:///C:/Docs/Sentencia%20T-1.pdf'), 'Sentencia T-1.pdf');
  assert.equal(displayName('https://ex.com/'), 'ex.com');
});
