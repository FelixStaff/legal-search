// Reading the PDF bytes and extracting per-page text with PDF.js.
// PDF.js runs with eval disabled and without scripting: embedded JavaScript in a
// PDF is never interpreted. The PDF is treated as untrusted input.

// Legacy build: same engine plus polyfills, so it runs on Chrome >= 116 (the modern
// build needs APIs such as Map.getOrInsertComputed that only very recent Chrome has).
import * as pdfjs from 'pdfjs-dist/legacy/build/pdf.mjs';
import type { PDFDocumentProxy } from 'pdfjs-dist/legacy/build/pdf.mjs';
import { assemblePageText } from './text.js';
import { hasPdfHeader } from './bytes.js';

/** Refuse absurdly large files instead of exhausting memory. */
export const MAX_PDF_BYTES = 400 * 1024 * 1024;

export class PdfReadError extends Error {
  constructor(public kind: 'fetch' | 'not_pdf' | 'too_large' | 'password' | 'parse', detail = '') {
    super(`${kind}${detail ? ': ' + detail : ''}`);
  }
}

pdfjs.GlobalWorkerOptions.workerSrc = chrome.runtime.getURL('pdf.worker.min.mjs');

function readFileUrl(url: string): Promise<ArrayBuffer> {
  // fetch() does not support file://; XHR does (with "Allow access to file URLs").
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open('GET', url);
    xhr.responseType = 'arraybuffer';
    xhr.onload = () =>
      xhr.response && (xhr.status === 0 || xhr.status === 200)
        ? resolve(xhr.response as ArrayBuffer)
        : reject(new PdfReadError('fetch', `status ${xhr.status}`));
    xhr.onerror = () => reject(new PdfReadError('fetch', 'xhr error'));
    xhr.send();
  });
}

export async function loadPdfBytes(url: string): Promise<ArrayBuffer> {
  let buf: ArrayBuffer;
  if (url.startsWith('file:')) {
    buf = await readFileUrl(url);
  } else {
    let res: globalThis.Response;
    try {
      // Same request the browser made to show the PDF; cookies included so
      // PDFs behind a login (court portals, intranets) can be read.
      res = await fetch(url, { credentials: 'include', cache: 'force-cache' });
    } catch (e) {
      throw new PdfReadError('fetch', String(e));
    }
    if (!res.ok) throw new PdfReadError('fetch', `HTTP ${res.status}`);
    const len = Number(res.headers.get('content-length') ?? '0');
    if (len > MAX_PDF_BYTES) throw new PdfReadError('too_large');
    buf = await res.arrayBuffer();
  }
  if (buf.byteLength > MAX_PDF_BYTES) throw new PdfReadError('too_large');
  if (!hasPdfHeader(new Uint8Array(buf, 0, Math.min(1024, buf.byteLength)))) throw new PdfReadError('not_pdf');
  return buf;
}

export interface OpenedPdf {
  doc: PDFDocumentProxy;
  /** Frees the worker-side document. Safe to call more than once. */
  close(): Promise<void>;
}

export async function openPdf(bytes: ArrayBuffer): Promise<OpenedPdf> {
  // PDF.js >= 6 never uses eval; the extension CSP (script-src 'self') would block it anyway.
  const task = pdfjs.getDocument({
    data: new Uint8Array(bytes),
    enableXfa: false,
    disableFontFace: true,
    useSystemFonts: false,
    stopAtErrors: false,
    verbosity: 0,
    cMapUrl: chrome.runtime.getURL('cmaps/'),
    cMapPacked: true,
  });
  try {
    const doc = await task.promise;
    return { doc, close: () => task.destroy() };
  } catch (e) {
    void task.destroy();
    const name = (e as { name?: string })?.name ?? '';
    if (name === 'PasswordException') throw new PdfReadError('password');
    throw new PdfReadError('parse', String(e));
  }
}

/** Extracts text of one page (1-based). Failure on one page yields empty text, not a crash. */
export async function pageText(doc: PDFDocumentProxy, n: number): Promise<string> {
  try {
    const page = await doc.getPage(n);
    const content = await page.getTextContent();
    page.cleanup();
    return assemblePageText(content.items as { str: string; hasEOL?: boolean }[]);
  } catch {
    return '';
  }
}
