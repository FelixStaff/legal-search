// Optional, local-only relevance feedback for evaluating retrieval later.
//
// Records are written ONLY when the user clicks a feedback button, are kept in
// chrome.storage.local (this browser profile), and leave the computer only if
// the user exports them to a file and shares that file. Nothing is uploaded.
//
// A record stores the query, the document hash, the ranked results (page,
// chunk index, score) and a short snippet of the judged passage — never the PDF.

export const FEEDBACK_FORMAT = 'legalsearch-feedback/1';
const KEY_PREFIX = 'fb:';
/** Snippet kept for the judged passage, so a reviewer can read what was judged. */
export const SNIPPET_CHARS = 300;

export type Judgement = 'relevant' | 'not_relevant';

export interface RankedResult {
  rank: number;
  page: number;
  chunkIndex: number;
  score: number;
}

/** Everything known about one search, captured when its results are shown. */
export interface SearchContext {
  searchId: string;
  timestamp: string;
  documentSha256: string;
  documentName: string;
  pageCount: number | null;
  query: string;
  retrievalMode: 'dense' | 'hybrid' | 'unknown';
  results: RankedResult[];
}

interface BaseRecord extends SearchContext {
  id: string;
  recordedAt: string;
  extensionVersion: string;
}

export interface ResultFeedback extends BaseRecord {
  type: 'result';
  judgement: Judgement;
  selected: RankedResult & { snippet: string };
}

export interface MissingFeedback extends BaseRecord {
  type: 'missing';
  /** Page where the user expected the answer, if they know it. */
  expectedPage: number | null;
  note: string;
}

export type FeedbackRecord = ResultFeedback | MissingFeedback;

/** Minimal subset of chrome.storage.StorageArea, injectable for tests. */
export interface KeyValueStore {
  get(keys: null): Promise<Record<string, unknown>>;
  set(items: Record<string, unknown>): Promise<void>;
  remove(keys: string[]): Promise<void>;
}

export function newSearchId(): string {
  return crypto.randomUUID();
}

export function resultFeedback(
  ctx: SearchContext,
  selected: RankedResult,
  passageText: string,
  judgement: Judgement,
  extensionVersion: string,
  now = new Date(),
): ResultFeedback {
  return {
    ...ctx,
    // One judgement per (search, passage): clicking again overwrites it.
    id: `${ctx.searchId}:${selected.chunkIndex}`,
    type: 'result',
    recordedAt: now.toISOString(),
    extensionVersion,
    judgement,
    selected: { ...selected, snippet: passageText.trim().slice(0, SNIPPET_CHARS) },
  };
}

export function missingFeedback(
  ctx: SearchContext,
  expectedPage: number | null,
  note: string,
  extensionVersion: string,
  now = new Date(),
): MissingFeedback {
  const page = expectedPage != null && Number.isInteger(expectedPage) && expectedPage >= 1 ? expectedPage : null;
  return {
    ...ctx,
    id: `${ctx.searchId}:missing`,
    type: 'missing',
    recordedAt: now.toISOString(),
    extensionVersion,
    expectedPage: page,
    note: note.trim().slice(0, 1000),
  };
}

export class FeedbackStore {
  constructor(private readonly store: KeyValueStore) {}

  async save(record: FeedbackRecord): Promise<void> {
    await this.store.set({ [KEY_PREFIX + record.id]: record });
  }

  async remove(id: string): Promise<void> {
    await this.store.remove([KEY_PREFIX + id]);
  }

  async all(): Promise<FeedbackRecord[]> {
    const items = await this.store.get(null);
    return Object.entries(items)
      .filter(([k]) => k.startsWith(KEY_PREFIX))
      .map(([, v]) => v as FeedbackRecord)
      .sort((a, b) => a.recordedAt.localeCompare(b.recordedAt) || a.id.localeCompare(b.id));
  }

  async count(): Promise<number> {
    return (await this.all()).length;
  }

  async clear(): Promise<void> {
    const items = await this.store.get(null);
    const keys = Object.keys(items).filter((k) => k.startsWith(KEY_PREFIX));
    if (keys.length) await this.store.remove(keys);
  }
}

/** The export file: self-describing JSON, one entry per judgement. */
export function exportJson(records: FeedbackRecord[], extensionVersion: string, now = new Date()): string {
  return JSON.stringify(
    {
      format: FEEDBACK_FORMAT,
      exportedAt: now.toISOString(),
      extensionVersion,
      recordCount: records.length,
      notes:
        'Local LegalSearch relevance feedback. Pages are 1-based; chunkIndex identifies the passage within the ' +
        'document index (identified by documentSha256). Scores are cosine similarities.',
      records,
    },
    null,
    2,
  );
}

export function exportFileName(now = new Date()): string {
  const d = now.toISOString().slice(0, 19).replace(/[:T]/g, '-');
  return `legalsearch-evaluation-${d}.json`;
}
