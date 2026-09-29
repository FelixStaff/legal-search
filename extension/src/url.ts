// URL helpers: PDF detection and #page=N navigation. Pure functions (unit-tested).

/** Schemes we can read a PDF from. */
export function isSupportedScheme(url: string): boolean {
  return /^(https?|file):/i.test(url);
}

/** True when the URL path clearly names a PDF (query/fragment ignored). */
export function looksLikePdfUrl(url: string): boolean {
  try {
    const u = new URL(url);
    if (!['http:', 'https:', 'file:'].includes(u.protocol)) return false;
    return /\.pdf$/i.test(decodeURIComponent(u.pathname));
  } catch {
    return false;
  }
}

/** The URL without its fragment: identifies "the same document" across page jumps. */
export function withoutFragment(url: string): string {
  const i = url.indexOf('#');
  return i === -1 ? url : url.slice(0, i);
}

/**
 * Returns `url` with its fragment replaced by `page=N`, preserving scheme, host,
 * path and query exactly. Other viewer parameters (zoom, view...) are dropped
 * on purpose so the jump is unambiguous.
 */
export function withPage(url: string, page: number): string {
  if (!Number.isInteger(page) || page < 1) throw new RangeError('page must be a positive integer');
  return `${withoutFragment(url)}#page=${page}`;
}

/** Human file name for display ("sentencia.pdf"), never used as a path. */
export function displayName(url: string): string {
  try {
    const u = new URL(url);
    const last = decodeURIComponent(u.pathname.split('/').filter(Boolean).pop() ?? '');
    return last || u.hostname || 'document.pdf';
  } catch {
    return 'document.pdf';
  }
}
