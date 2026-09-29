// Assembles readable page text from PDF.js text items. Pure (unit-tested).

export interface TextItemLike {
  str: string;
  hasEOL?: boolean;
}

/**
 * Joins PDF.js items into a single string per page:
 * - items separated by EOL become separate lines;
 * - a word hyphenated at line end ("conoci-" + "miento") is re-joined;
 * - whitespace is collapsed.
 */
export function assemblePageText(items: TextItemLike[]): string {
  let out = '';
  for (const it of items) {
    if (typeof it.str !== 'string') continue;
    out += it.str;
    if (it.hasEOL) out += '\n';
  }
  return out
    .replace(/(\p{L})-\n(\p{Ll})/gu, '$1$2') // de-hyphenate across line breaks
    .replace(/\s+/g, ' ')
    .trim();
}

/** Groups pages into batches whose total text stays under `maxChars`. */
export function batchPages<T extends { text: string }>(pages: T[], maxChars: number): T[][] {
  const batches: T[][] = [];
  let cur: T[] = [];
  let size = 0;
  for (const p of pages) {
    if (cur.length && size + p.text.length > maxChars) {
      batches.push(cur);
      cur = [];
      size = 0;
    }
    cur.push(p);
    size += p.text.length;
  }
  if (cur.length) batches.push(cur);
  return batches;
}

/** Short snippet for the results list. */
export function snippet(text: string, max = 420): string {
  const t = text.trim();
  if (t.length <= max) return t;
  const cut = t.lastIndexOf(' ', max);
  return t.slice(0, cut > max * 0.6 ? cut : max) + '…';
}
