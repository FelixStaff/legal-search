import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  exportFileName, exportJson, FEEDBACK_FORMAT, FeedbackStore, missingFeedback, newSearchId, resultFeedback,
  SNIPPET_CHARS, type KeyValueStore, type SearchContext,
} from '../src/feedback.js';

class MemoryStore implements KeyValueStore {
  data = new Map<string, unknown>();
  async get(_: null) { return Object.fromEntries(this.data); }
  async set(items: Record<string, unknown>) { for (const [k, v] of Object.entries(items)) this.data.set(k, structuredClone(v)); }
  async remove(keys: string[]) { for (const k of keys) this.data.delete(k); }
}

const ctx = (over: Partial<SearchContext> = {}): SearchContext => ({
  searchId: 'S1',
  timestamp: '2026-09-28T10:00:00.000Z',
  documentSha256: 'a'.repeat(64),
  documentName: 'ruling.pdf',
  pageCount: 12,
  query: 'the State knew about the risk',
  retrievalMode: 'dense',
  results: [
    { rank: 1, page: 3, chunkIndex: 2, score: 0.86 },
    { rank: 2, page: 12, chunkIndex: 11, score: 0.84 },
  ],
  ...over,
});
const T = new Date('2026-09-28T10:00:05.000Z');

test('result feedback captures everything needed for evaluation, not the PDF', () => {
  const long = 'x'.repeat(5000);
  const r = resultFeedback(ctx(), { rank: 1, page: 3, chunkIndex: 2, score: 0.86 }, long, 'relevant', '1.1.0', T);
  assert.equal(r.type, 'result');
  assert.equal(r.id, 'S1:2');
  assert.equal(r.documentSha256, 'a'.repeat(64));
  assert.equal(r.query, 'the State knew about the risk');
  assert.equal(r.retrievalMode, 'dense');
  assert.deepEqual(r.results.map((x) => x.chunkIndex), [2, 11]);
  assert.equal(r.selected.page, 3);
  assert.equal(r.judgement, 'relevant');
  assert.equal(r.recordedAt, T.toISOString());
  assert.equal(r.selected.snippet.length, SNIPPET_CHARS, 'only a short snippet is kept');
});

test('missing-result feedback validates the expected page', () => {
  assert.equal(missingFeedback(ctx(), 7, '  art. 25  ', '1.1.0', T).expectedPage, 7);
  assert.equal(missingFeedback(ctx(), 7, '  art. 25  ', '1.1.0', T).note, 'art. 25');
  for (const bad of [0, -3, 2.5, Number.NaN, null]) assert.equal(missingFeedback(ctx(), bad, '', '1.1.0', T).expectedPage, null);
  assert.equal(missingFeedback(ctx(), null, 'n'.repeat(5000), '1.1.0', T).note.length, 1000);
  assert.equal(missingFeedback(ctx(), null, '', '1.1.0', T).id, 'S1:missing');
});

test('store: one judgement per passage per search, toggle and clear', async () => {
  const mem = new MemoryStore();
  mem.data.set('unrelated-key', 1); // other extension data must be left alone
  const s = new FeedbackStore(mem);
  const sel = { rank: 1, page: 3, chunkIndex: 2, score: 0.86 };
  await s.save(resultFeedback(ctx(), sel, 'text', 'relevant', '1.1.0', T));
  await s.save(resultFeedback(ctx(), sel, 'text', 'not_relevant', '1.1.0', T)); // overwrite
  await s.save(resultFeedback(ctx({ searchId: 'S2' }), sel, 'text', 'relevant', '1.1.0', T));
  await s.save(missingFeedback(ctx(), 5, '', '1.1.0', T));
  const all = await s.all();
  assert.equal(all.length, 3);
  assert.equal(all.find((r) => r.id === 'S1:2')?.type === 'result' && (all.find((r) => r.id === 'S1:2') as any).judgement, 'not_relevant');
  await s.remove('S2:2');
  assert.equal(await s.count(), 2);
  await s.clear();
  assert.equal(await s.count(), 0);
  assert.equal(mem.data.get('unrelated-key'), 1);
});

test('export is self-describing JSON', async () => {
  const s = new FeedbackStore(new MemoryStore());
  await s.save(resultFeedback(ctx(), { rank: 2, page: 12, chunkIndex: 11, score: 0.84 }, 'p', 'not_relevant', '1.1.0', T));
  const j = JSON.parse(exportJson(await s.all(), '1.1.0', T));
  assert.equal(j.format, FEEDBACK_FORMAT);
  assert.equal(j.recordCount, 1);
  assert.equal(j.records[0].selected.chunkIndex, 11);
  assert.equal(j.exportedAt, T.toISOString());
  assert.match(exportFileName(T), /^legalsearch-evaluation-2026-09-28-10-00-05\.json$/);
});

test('search ids are unique', () => {
  const ids = new Set(Array.from({ length: 100 }, newSearchId));
  assert.equal(ids.size, 100);
});
