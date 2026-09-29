// Side panel controller: detects the PDF in the active tab, gets it indexed by
// the local engine (or reuses the cache), runs searches and navigates to pages.

import { HostError, HostUnavailable, type PageText, type SearchHit } from './messages.js';
import { NativeClient, type PortLike } from './native.js';
import { loadPdfBytes, openPdf, pageText, PdfReadError, type OpenedPdf } from './pdf.js';
import { sha256Hex } from './bytes.js';
import { S } from './strings.js';
import { batchPages, snippet } from './text.js';
import { displayName, isSupportedScheme, looksLikePdfUrl, withPage, withoutFragment } from './url.js';
import { HOST_NAME } from './config.js';
import {
  exportFileName, exportJson, FeedbackStore, missingFeedback, newSearchId, resultFeedback,
  type Judgement, type KeyValueStore, type SearchContext,
} from './feedback.js';

const K = 8;
/** Page text sent per native message (chars). Keeps each message ~1-2 MB. */
const BATCH_CHARS = 400_000;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const el = {
  doc: $('doc'),
  form: $<HTMLFormElement>('search-form'),
  query: $<HTMLInputElement>('query'),
  button: $<HTMLButtonElement>('search-btn'),
  status: $('status'),
  statusText: $('status-text'),
  statusDetail: $('status-detail'),
  progress: $('progress'),
  progressBar: $('progress-bar'),
  action: $<HTMLButtonElement>('status-action'),
  results: $('results'),
  resultsTitle: $('results-title'),
  missing: $('missing'),
  missingOpen: $<HTMLButtonElement>('missing-open'),
  missingForm: $<HTMLFormElement>('missing-form'),
  missingPage: $<HTMLInputElement>('missing-page'),
  missingNote: $<HTMLInputElement>('missing-note'),
  missingCancel: $<HTMLButtonElement>('missing-cancel'),
  missingDone: $('missing-done'),
  exportBtn: $<HTMLButtonElement>('export-btn'),
  deleteBtn: $<HTMLButtonElement>('delete-btn'),
};

const VERSION = chrome.runtime.getManifest().version;
const feedback = new FeedbackStore(chrome.storage.local as unknown as KeyValueStore);
/** The search whose results are on screen (for feedback). */
let lastSearch: SearchContext | null = null;

const client = new NativeClient(
  () => chrome.runtime.connectNative(HOST_NAME) as unknown as PortLike,
  () => chrome.runtime.lastError?.message,
);

type DocState =
  | { phase: 'none' }
  | { phase: 'loading'; tabId: number; key: string }
  | { phase: 'indexing'; tabId: number; key: string; sha256: string }
  | { phase: 'ready'; tabId: number; key: string; sha256: string; name: string; pageCount: number }
  | { phase: 'error'; tabId: number; key: string };

let state: DocState = { phase: 'none' };
/** Incremented whenever the panel switches document; stale async work checks it and stops. */
let generation = 0;

// ------------------------------------------------------------------ view

type Tone = 'info' | 'ok' | 'warn' | 'error';

function showStatus(
  text: string,
  opts: { tone?: Tone; detail?: string; pct?: number | null; busy?: boolean; action?: { label: string; run: () => void } } = {},
) {
  el.status.hidden = false;
  el.status.dataset.tone = opts.tone ?? 'info';
  el.status.classList.toggle('busy', !!opts.busy);
  el.statusText.textContent = text;
  el.statusDetail.textContent = opts.detail ?? '';
  el.statusDetail.hidden = !opts.detail;
  const pct = opts.pct;
  el.progress.hidden = pct == null;
  if (pct != null) {
    el.progressBar.style.width = `${Math.max(0, Math.min(100, pct))}%`;
    el.progress.setAttribute('aria-valuenow', String(Math.round(pct)));
  }
  el.action.hidden = !opts.action;
  el.action.onclick = opts.action ? opts.action.run : null;
  if (opts.action) el.action.textContent = opts.action.label;
}

function setSearchEnabled(on: boolean) {
  el.query.disabled = !on;
  el.button.disabled = !on;
}

function clearResults() {
  el.results.replaceChildren();
  el.resultsTitle.hidden = true;
  el.missing.hidden = true;
  lastSearch = null;
}

// ------------------------------------------------------------- feedback

async function refreshFeedbackBar() {
  try {
    const n = await feedback.count();
    el.exportBtn.textContent = S.exportData(n);
    el.exportBtn.disabled = n === 0;
    el.deleteBtn.hidden = n === 0;
  } catch {
    el.exportBtn.disabled = true;
  }
}

async function judge(ctx: SearchContext, h: SearchHit, rank: number, j: Judgement, buttons: HTMLButtonElement[]) {
  const pressed = buttons.find((b) => b.dataset.j === j)?.getAttribute('aria-pressed') === 'true';
  const rec = resultFeedback(ctx, { rank, page: h.page, chunkIndex: h.chunkIndex, score: h.score }, h.text, j, VERSION);
  try {
    if (pressed) await feedback.remove(rec.id); // clicking the active choice again clears it
    else await feedback.save(rec);
    for (const b of buttons) b.setAttribute('aria-pressed', String(!pressed && b.dataset.j === j));
  } catch {
    showStatus(S.feedbackFailed, { tone: 'warn' });
  }
  void refreshFeedbackBar();
}

function feedbackButtons(ctx: SearchContext, h: SearchHit, rank: number): HTMLElement {
  const box = document.createElement('div');
  box.className = 'feedback';
  const buttons: HTMLButtonElement[] = [];
  for (const [j, label] of [['relevant', S.relevant], ['not_relevant', S.notRelevant]] as const) {
    const b = document.createElement('button');
    b.type = 'button';
    b.textContent = label;
    b.dataset.j = j;
    if (j === 'not_relevant') b.className = 'neg';
    b.setAttribute('aria-pressed', 'false');
    b.addEventListener('click', () => void judge(ctx, h, rank, j, buttons));
    buttons.push(b);
  }
  box.append(...buttons);
  return box;
}

async function saveMissing() {
  if (!lastSearch) return;
  const page = el.missingPage.value ? Number(el.missingPage.value) : null;
  try {
    await feedback.save(missingFeedback(lastSearch, page, el.missingNote.value, VERSION));
    el.missingForm.hidden = true;
    el.missingDone.textContent = S.feedbackSaved;
    el.missingDone.hidden = false;
  } catch {
    showStatus(S.feedbackFailed, { tone: 'warn' });
  }
  void refreshFeedbackBar();
}

async function exportFeedback() {
  const records = await feedback.all();
  if (!records.length) return;
  // A local file download: the data goes wherever the user saves it, nowhere else.
  const url = URL.createObjectURL(new Blob([exportJson(records, VERSION)], { type: 'application/json' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = exportFileName();
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

async function deleteFeedback() {
  if (!confirm(S.deleteConfirm)) return;
  await feedback.clear();
  for (const b of document.querySelectorAll('.feedback button')) b.setAttribute('aria-pressed', 'false');
  void refreshFeedbackBar();
}

function renderResults(hits: SearchHit[], tabId: number, ctx: SearchContext) {
  clearResults();
  lastSearch = ctx;
  el.resultsTitle.hidden = false;
  el.missing.hidden = false;
  el.missingOpen.hidden = false;
  el.missingForm.hidden = true;
  el.missingDone.hidden = true;
  if (!hits.length) {
    const p = document.createElement('p');
    p.className = 'empty';
    p.textContent = S.noResults;
    el.results.append(p);
    return;
  }
  hits.forEach((h, i) => {
    const li = document.createElement('li');
    li.className = 'result';
    const head = document.createElement('div');
    head.className = 'result-page';
    head.textContent = S.page(h.page);
    const quote = document.createElement('blockquote');
    quote.textContent = `“${snippet(h.text)}”`; // textContent: PDF text is never interpreted as HTML
    quote.title = `${Math.round(h.score * 100)} %`;
    const go = document.createElement('button');
    go.type = 'button';
    go.className = 'go';
    go.textContent = S.goToPage;
    go.addEventListener('click', () => void goToPage(tabId, h.page));
    const actions = document.createElement('div');
    actions.className = 'result-actions';
    actions.append(go, feedbackButtons(ctx, h, i + 1));
    li.append(head, quote, actions);
    li.addEventListener('dblclick', (ev) => {
      if (!(ev.target instanceof HTMLButtonElement)) void goToPage(tabId, h.page);
    });
    el.results.append(li);
  });
}

// ------------------------------------------------------------- navigation

async function goToPage(tabId: number, page: number) {
  try {
    const tab = await chrome.tabs.get(tabId);
    if (!tab.url) return;
    // Chrome's PDF viewer reads #page=N only when the document loads; a
    // fragment-only change is ignored (verified in scripts/e2e-chrome.mjs). So:
    // set the fragment, then reload. The panel keeps its index because the URL
    // without fragment (the document key) does not change.
    const target = withPage(tab.url, page);
    if (tab.url !== target) {
      await chrome.tabs.update(tabId, { url: target, active: true });
      // Reloading before the fragment change commits would reload the old URL.
      for (let i = 0; i < 40; i++) {
        if ((await chrome.tabs.get(tabId)).url === target) break;
        await new Promise((r) => setTimeout(r, 25));
      }
    }
    await chrome.tabs.reload(tabId);
  } catch {
    /* tab closed: nothing to do */
  }
}

// --------------------------------------------------------------- errors

function showError(e: unknown, retry: () => void) {
  if (e instanceof HostUnavailable) {
    if (e.notInstalled) showStatus(S.engineMissing, { tone: 'error', detail: S.engineMissingHelp, action: { label: S.retry, run: retry } });
    else showStatus(S.genericError, { tone: 'error', action: { label: S.retry, run: retry } });
    return;
  }
  if (e instanceof HostError) {
    switch (e.code) {
      case 'no_text':
        return showStatus(S.noText, { tone: 'warn', detail: S.scanned });
      case 'model_missing':
        return showStatus(S.engineMissing, { tone: 'error', detail: S.engineMissingHelp, action: { label: S.retry, run: retry } });
      case 'indexing_in_progress':
        return showStatus(S.indexingBusy, { tone: 'info', busy: true });
      default:
        return showStatus(S.genericError, { tone: 'error', action: { label: S.retry, run: retry } });
    }
  }
  if (e instanceof PdfReadError) {
    if (e.kind === 'password') return showStatus(S.passwordProtected, { tone: 'warn' });
    return showStatus(S.cantRead, { tone: 'error', action: { label: S.retry, run: retry } });
  }
  showStatus(S.genericError, { tone: 'error', action: { label: S.retry, run: retry } });
}

// ------------------------------------------------------------ document

async function refresh(force = false) {
  let tab: chrome.tabs.Tab | undefined;
  try {
    [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  } catch {
    return;
  }
  if (!tab?.id || !tab.url) {
    setNoDocument(S.notPdf);
    return;
  }
  const key = withoutFragment(tab.url);
  // Same document (e.g. only #page changed after "Go to page"): keep everything.
  if (!force && state.phase !== 'none' && state.tabId === tab.id && state.key === key) return;
  await loadDocument(tab.id, tab.url, force);
}

function setNoDocument(message: string, action?: { label: string; run: () => void }) {
  generation++;
  state = { phase: 'none' };
  el.doc.textContent = '';
  setSearchEnabled(false);
  clearResults();
  showStatus(message, { tone: 'info', action });
}

async function loadDocument(tabId: number, url: string, force: boolean) {
  const key = withoutFragment(url);
  if (!isSupportedScheme(url)) return setNoDocument(S.unsupportedPage);
  if (!looksLikePdfUrl(url) && !force) {
    return setNoDocument(S.notPdf, { label: S.tryAnyway, run: () => void loadDocument(tabId, url, true) });
  }
  const gen = ++generation;
  const stale = () => gen !== generation;
  state = { phase: 'loading', tabId, key };
  el.doc.textContent = displayName(url);
  setSearchEnabled(false);
  clearResults();
  const retry = () => void loadDocument(tabId, url, true);

  if (url.startsWith('file:') && !(await chrome.extension.isAllowedFileSchemeAccess())) {
    state = { phase: 'error', tabId, key };
    showStatus(S.fileAccessNeeded, {
      tone: 'warn',
      detail: S.fileAccessSteps,
      action: { label: S.openSettings, run: () => void chrome.tabs.create({ url: `chrome://extensions/?id=${chrome.runtime.id}` }) },
    });
    return;
  }

  showStatus(S.preparing, { busy: true, pct: null });
  let pdf: OpenedPdf | null = null;
  try {
    const bytes = await loadPdfBytes(url);
    if (stale()) return;
    const sha256 = await sha256Hex(bytes);
    pdf = await openPdf(bytes);
    if (stale()) return;
    const pageCount = pdf.doc.numPages;
    const filename = displayName(url);

    const opened = await client.request({ type: 'open', sha256, filename, pageCount });
    if (stale()) return;
    if (opened.type === 'opened' && opened.status === 'ready') {
      return setReady(tabId, key, sha256, filename, pageCount);
    }

    // ---- first time: extract text page by page, streaming batches to the engine
    state = { phase: 'indexing', tabId, key, sha256 };
    await client.request({ type: 'index_begin', sha256, filename, pageCount });
    let pending: PageText[] = [];
    let pendingChars = 0;
    const flush = async () => {
      for (const batch of batchPages(pending, BATCH_CHARS)) {
        await client.request({ type: 'index_pages', sha256, pages: batch });
      }
      pending = [];
      pendingChars = 0;
    };
    for (let n = 1; n <= pageCount; n++) {
      if (stale()) return;
      const text = await pageText(pdf.doc, n);
      pending.push({ page: n, text });
      pendingChars += text.length;
      if (pendingChars >= BATCH_CHARS) await flush();
      showStatus(S.extracting, { busy: true, pct: (n / pageCount) * 100, detail: `${Math.round((n / pageCount) * 100)} %` });
    }
    await flush();
    await pdf.close();
    pdf = null;
    if (stale()) return;

    showStatus(S.indexing, { busy: true, pct: 0, detail: '0 %' });
    await client.request({ type: 'index_end', sha256 }, (p) => {
      if (!stale()) showStatus(S.indexing, { busy: true, pct: p.pct, detail: `${p.pct} %` });
    });
    if (stale()) return;
    setReady(tabId, key, sha256, filename, pageCount);
  } catch (e) {
    if (stale()) return;
    state = { phase: 'error', tabId, key };
    showError(e, retry);
  } finally {
    void pdf?.close();
  }
}

function setReady(tabId: number, key: string, sha256: string, name: string, pageCount: number) {
  state = { phase: 'ready', tabId, key, sha256, name, pageCount };
  showStatus(S.ready, { tone: 'ok' });
  setSearchEnabled(true);
  el.query.focus();
}

// --------------------------------------------------------------- search

async function search() {
  const query = el.query.value.trim();
  if (!query) return;
  if (state.phase === 'indexing' || state.phase === 'loading') {
    showStatus(S.indexingBusy, { busy: true });
    return;
  }
  if (state.phase !== 'ready') return;
  const { tabId, sha256, name, pageCount } = state;
  const gen = generation;
  el.button.disabled = true;
  showStatus(S.searching, { busy: true });
  try {
    const res = await client.request({ type: 'search', sha256, query: query.slice(0, 1000), k: K });
    if (gen !== generation) return;
    if (res.type === 'results') {
      const ctx: SearchContext = {
        searchId: newSearchId(),
        timestamp: new Date().toISOString(),
        documentSha256: sha256,
        documentName: name,
        pageCount,
        query,
        retrievalMode: res.mode ?? 'unknown',
        results: res.results.map((h, i) => ({ rank: i + 1, page: h.page, chunkIndex: h.chunkIndex, score: h.score })),
      };
      renderResults(res.results, tabId, ctx);
      showStatus(S.ready, { tone: 'ok' });
    }
  } catch (e) {
    if (gen === generation) showError(e, () => void search());
  } finally {
    if (gen === generation) el.button.disabled = false;
  }
}

// ----------------------------------------------------------------- wiring

el.form.addEventListener('submit', (ev) => {
  ev.preventDefault();
  void search();
});
chrome.tabs.onActivated.addListener(() => void refresh());
chrome.tabs.onUpdated.addListener((tabId, info) => {
  if (info.url !== undefined || info.status === 'complete') {
    void chrome.tabs.query({ active: true, currentWindow: true }).then(([t]) => {
      if (t?.id === tabId) void refresh();
    });
  }
});
el.missingOpen.addEventListener('click', () => {
  el.missingOpen.hidden = true;
  el.missingForm.hidden = false;
  el.missingPage.value = '';
  el.missingNote.value = '';
  el.missingPage.focus();
});
el.missingCancel.addEventListener('click', () => {
  el.missingForm.hidden = true;
  el.missingOpen.hidden = false;
});
el.missingForm.addEventListener('submit', (ev) => {
  ev.preventDefault();
  void saveMissing();
});
el.exportBtn.addEventListener('click', () => void exportFeedback());
el.deleteBtn.addEventListener('click', () => void deleteFeedback());
setSearchEnabled(false);
void refreshFeedbackBar();
void refresh();
