# LegalSearch architecture

## Components

| Component | Technology | Responsibility |
|---|---|---|
| Chrome extension (`extension/`) | Manifest V3, TypeScript, Side Panel, PDF.js 6 | Detect the PDF in the active tab, read its bytes, compute the SHA-256, extract text per page, show progress, send queries, show results and navigate to the page |
| Local engine (`native/`, `LegalSearch.exe`) | Rust, ONNX Runtime 1.28 (CPU, loaded dynamically), SQLite | Per-page chunking, tokenization, embeddings, index, Top-K search, optional BM25/RRF, persistent cache, Native Messaging protocol |
| Installer (`installer/`) | Inno Setup 6, per-user | Copy the engine, runtime, model and extension, and register the Native Messaging host in HKCU |

## Actual flow

```
Side panel                                      LegalSearch.exe (1 process per open panel)
──────────                                      ──────────────────────────────────────────
active tab → URL (without #fragment = key)
fetch / XHR(file://) → bytes → "%PDF-" ?
SHA-256(bytes) · PDF.js → numPages
open{sha256, pageCount}  ─────────────────────► in SQLite with this model?
                         ◄─────────────────────  opened{ready | needs_index}
[needs_index]
index_begin              ─────────────────────►
for each page: getTextContent → text
index_pages{≤400k chars} ─────────────────────► accumulate pages (limits validated)
index_end                ─────────────────────► text layer? (if not: error no_text)
                                                 per-page chunking (≤200 tokens, 40 overlap)
                         ◄───────── progress %  embeddings, 32 passages per progress step
                         ◄─────────────────────  ready  (index stored in SQLite)
search{query, k=8}       ─────────────────────► u = f("query: "+q) ; s = V·û ; Top-K
                         ◄─────────────────────  results[{page, text, score}]
"Go to page": tabs.update(url#page=N) → tabs.reload
```

## Semantic retrieval

- **Model**: `intfloat/multilingual-e5-small` (MIT), exported to ONNX with INT8 dynamic quantization
  (`Xenova/multilingual-e5-small`, `onnx/model_quantized.onnx`, 118 MB, 384 dimensions).
- **Encoding**: e5 prefixes (`query: ` for the query, `passage: ` for passages), attention-masked mean pooling and
  L2 normalization. At most 512 tokens per text.
- **Search**: in-memory matrix `V ∈ ℝ^{n×384}` (normalized vectors), `s = V·û` (dot product = cosine), and Top-K by
  partial selection (`select_nth_unstable`). Exact search, no approximate index: 300 pages are about 500 passages and a
  full search takes about 3 ms including the round trip to Chrome.
- **Embeddings are the primary signal.** There are no regexes, legal rules, synonym lists or keyword-only search. BM25
  exists only as an **optional complementary lexical signal** (see "Hybrid retrieval") and is **off by default** in
  this version. The only textual "rule" is structural: text is split at sentence ends to form passages.

### Model choice (evaluated)

Evaluated with 16 Spanish queries on the test PDF (`tests/fixtures/corpus.json`). One of them shares no word with
the target passage, and the test checks that automatically.

| Model | Size | recall@1 | recall@3 | Latency per query |
|---|---|---|---|---|
| **e5-small INT8** (chosen) | 118 MB | **12/16** | 13/16 | ~3 ms |
| e5-base INT8 | 278 MB | 11/16 | **15/16** | ~6 ms |

On the first set of 8 queries, e5-small FP32 (470 MB) scored recall@1 5/8 and recall@3 6/8 versus 5/8 and 5/8 for
INT8: quantization does not noticeably hurt quality. e5-base improves recall@3 but worsens @1 and is 2.4× larger in
size and RAM, so **e5-small** (the initial preference) is kept. It can be built with e5-base without code changes:
`scripts\fetch-model.ps1 -Model base`, then `build-all`. The cache invalidates itself because its key includes
`model_id.txt`.

### Chunking

- Each page is processed on its own: **a passage never mixes pages** (covered by tests).
- Text is split into sentences (`. ? ! ; …`) and packed up to **200 real model tokens**, with **~40 tokens of
  overlap** (whole sentences repeated). Sentences longer than 200 tokens are cut by words.
- Passages with fewer than 12 alphanumeric characters (stray page numbers) are dropped.
- Parameters live in `ChunkConfig` (`native/src/chunker.rs`). Changing them requires bumping `PIPELINE_VERSION`
  (`native/src/embed.rs`) so the cache is rebuilt.

### Compact tokenizer

Hugging Face's `tokenizers` crate keeps **~234 MB** of heap for XLM-R's 250,000-piece Unigram vocabulary because its
trie uses one `HashMap` per node. `native/src/tokenizer.rs` implements the same pipeline (special tokens,
SentencePiece *Precompiled* normalizer via the `spm_precompiled` crate, space collapsing, Metaspace and Unigram Viterbi
with *fuse_unk*) storing the pieces in a single arena with binary search: **~6 MB**. `tests/tokenizer_equiv.rs`
checks that it produces **identical IDs** to `tokenizers` on 648 texts (corpus, edge cases and fuzzing across 10
scripts). `tokenizers` is only a development dependency.

### Determinism

INT8 dynamic quantization computes activation scales per tensor, so padding from other texts in a batch slightly
changed the vectors. A **batch size of 1** (no padding) is used: every vector depends only on its own text.

## Hybrid retrieval (dense + BM25, RRF): implemented, off by default

Rules: `EMBEDDINGS = PRIMARY_RETRIEVAL_SIGNAL`, `BM25 = ALLOWED_AS_COMPLEMENTARY_LEXICAL_SIGNAL`,
`REGEX_AS_RELEVANCE_ENGINE = FORBIDDEN`, `KEYWORD_ONLY_SEARCH = FORBIDDEN`.

```
query ─┬─ dense: e5("query: "+q) · V  → Top-40 ─┐
       └─ BM25 over the passages      → Top-40 ─┴─ RRF → Top-8
RRF(d) = 1/(k + r_dense(d)) + λ · 1/(k + r_bm25(d))
```

- **BM25** (`native/src/lexical.rs`): own in-memory implementation, no dependencies, `k1 = 1.2`, `b = 0.75` and the
  non-negative IDF `ln(1 + (N − n + 0.5)/(n + 0.5))`. Deterministic tokenization: lowercase, Latin diacritics removed
  (ñ is kept), alphanumeric runs, and `. / - ,` between digits stay inside the token (`8.1`, `1234/2020`,
  `2014-00871`, `12.000`). Single letters are dropped, single digits are kept. No stopwords, no stemming, no synonyms.
- **Index**: rebuilt in memory from `chunks.text`, which the cache already stores, on the first search of each
  document (494 passages: **~289 KB, 5 ms**). **The on-disk format does not change.**
- **Fusion** (`native/src/hybrid.rs`): RRF over the Top-40 candidates of each list; ties are broken by cosine. The
  `score` the extension receives is still the cosine, and the interface is unchanged.
- **Fallback**: if the query has no useful terms (or none occurs in the document) the result is exactly the dense
  Top-K. Any failure building or querying BM25 is caught (`catch_unwind`), logged, and the search continues dense-only.
- **Enabling**: `DEFAULT_HYBRID.enabled` in `hybrid.rs`, or the environment variable `LEGALSEARCH_HYBRID=1`
  (evaluation only). The candidate configuration is `CANDIDATE_HYBRID` (k = 10, λ = 0.25).

### Evaluation (`native/tests/retrieval_eval.rs`)

54 queries: the 16 semantic fixture queries plus 38 on `tests/fixtures/eval_corpus.json` (a fictitious 21-page
Spanish judgment), split into paraphrases, terms, articles, cases, dates, names, numbers and mixed queries. Queries
and their correct pages were written before looking at any result. Metrics are page-level over the Top-8.

| System | R@1 | R@3 | R@5 | MRR | Semantic MRR | Lexical MRR |
|---|---|---|---|---|---|---|
| Dense only | 87.0% | 92.6% | 98.1% | 0.913 | 0.890 | 0.935 |
| BM25 only | 74.1% | 81.5% | 83.3% | 0.792 | 0.567 | 1.000 |
| Hybrid k=60, λ=0.5 (requested initial values) | 79.6% | 90.7% | 96.3% | 0.865 | **0.748** | 0.973 |
| Hybrid k=10, λ=0.25 (best of the grid) | 90.7% | 92.6% | 98.1% | 0.929 | 0.881 | 0.973 |

- With k = 60 the differences between dense ranks are tiny (1/61 − 1/80 ≈ 0.004) compared with the bonus for being
  BM25's #1 (λ/61 ≈ 0.008). BM25 then drives the order and **degrades semantic queries**.
- With k = 10 and λ = 0.25 the dense signal dominates: 4 queries improve (including `artículo 46.1.a`,
  `45.000 dólares` and `reconocimiento público…`) and 2 semantic ones get worse (5→6 and 2→5).
- **Evaluation limits**: the parameters were chosen on the same 54 queries (in-sample), each corpus page is a single
  passage, and dense retrieval already handled most lexical queries in such a short document. It must be validated on
  long real PDFs and lawyers' queries before turning it on by default.
- In-process latency: dense ~2 ms (embedding included), BM25 ~0.004 ms, fusion ~0.002 ms. End to end over Native
  Messaging, ~3 ms in both modes.

## Cache (SQLite)

`%LOCALAPPDATA%\LegalSearch\data\index.sqlite`

```sql
documents(id, sha256 UNIQUE, filename, page_count, chunk_count, model_version, dim, indexed_at, last_used_at)
chunks(document_id → documents ON DELETE CASCADE, chunk_index, page, text, embedding BLOB /* f32 LE */)
```

- Key: **SHA-256 of the PDF bytes**. The same file under another name or URL reuses the index.
- `model_version` = `model_id.txt` + `PIPELINE_VERSION`. If the model or chunking changes, indexes are ignored and rebuilt.
- On-disk format version: `PRAGMA user_version` = `SCHEMA_VERSION` (1). A database with any other non-zero version is
  dropped and recreated (the cache is regenerable), so an old format is never misread. Version 0 means databases
  created by 1.0.0, which have the same layout and are adopted as is. BM25 stores nothing on disk.
- The 300 most recently used documents are kept (LRU). A corrupt BLOB counts as a cache miss.
- The cache is **regenerable**; deleting it only forces re-indexing.

## Native Messaging protocol

JSON messages with a little-endian u32 length prefix over stdin/stdout (`native/src/protocol.rs`,
`extension/src/messages.ts`). Every request carries an `id` that is echoed in its response.

| Request | Response |
|---|---|
| `hello` | `hello{version, protocol}` |
| `open{sha256, filename, pageCount}` | `opened{status: ready\|needs_index, chunkCount}` |
| `index_begin{sha256, filename, pageCount}` | `ack` |
| `index_pages{sha256, pages[{page,text}]}` | `ack` |
| `index_end{sha256}` | `progress{done,total,pct}`* → `ready{chunkCount}` |
| `search{sha256, query, k?}` | `results[{page, chunkIndex, text, score}]` |
| any error | `error{code}`: `invalid_request`, `not_indexed`, `no_text`, `indexing_in_progress`, `model_missing`, `internal` |

The extension maps the codes to simple user messages. Technical details only go to the local log.

## Lifecycle and resources

- Chrome starts `LegalSearch.exe` when the panel connects (`connectNative`) and the process **exits when the panel
  closes** (EOF on stdin). No service, scheduled task, port or resident process.
- `onnxruntime.dll` and the model are loaded **only when needed** (when indexing, or when opening an already cached
  document so the first query is instant). A panel open on a tab without a PDF uses about 5 MB.
- Official Microsoft ONNX Runtime (CPU), loaded by **absolute path** from the exe folder, never through the DLL search
  order. The exe uses a static CRT; `onnxruntime.dll` ships with the VC++ runtime DLLs next to it (app-local deployment).
- Inference threads: `min(4, cores)`.

## "Go to page"

Chrome's PDF viewer **only applies `#page=N` when the document loads**: a fragment-only change does nothing
(verified in `scripts/e2e-chrome.mjs`). So the extension sets the fragment with `chrome.tabs.update`, waits for it
to commit and calls `chrome.tabs.reload`. The original URL is preserved and only the fragment is replaced
(`withPage`, tested with `https://` and `file:///C:/…` including spaces and accents). The panel's document key is the
URL without fragment, so the reload does not trigger re-indexing.

## Security

- The host's `allowed_origins` = **only** the LegalSearch extension ID. The exe also rejects any caller whose first
  argument is not a `chrome-extension://` origin.
- Strict validation: `serde(deny_unknown_fields)`, closed types, SHA-256 = 64 lowercase hex, 1–1000 character query,
  pages 1–20,000, 200,000 characters per page and 60 M in total, messages up to 8 MB (a larger header stops the process
  without allocating), `k ≤ 50`.
- The host **never opens paths received in messages**: every path derives from the exe location and `%LOCALAPPDATA%`.
  The file name is display-only and sanitized. No path traversal is possible.
- It runs no commands, shell or PowerShell. Nothing in the PDF is interpreted: the extension uses PDF.js 6 (no `eval`)
  with XFA and scripting disabled, and the extension CSP (`script-src 'self'; object-src 'none'`) blocks any eval.
- PDF text is inserted into the panel with `textContent`, never as HTML.
- Panics are caught and answered with `internal`, with no stack traces reaching the UI.

## Privacy

No component makes network requests during use. Queries are stored only if the user gives optional feedback, and
then only in the local Chrome profile (see above). The extension only reads the PDF the user already opened (the same
URL, from the browser cache when available) and talks only to the local process. The model and runtime are downloaded
**only when building the installer**. No analytics, telemetry or error reporting. The local log
(`logs\legalsearch.log`, rotated at 2 × 512 KB) records events and timings, **never** document text or queries.

## Local relevance feedback (optional)

`extension/src/feedback.ts`. When the user clicks **Relevant** / **Not relevant** on a result, or reports a
**Missing result**, one record is written to `chrome.storage.local` (key `fb:<searchId>:<chunkIndex|missing>`; one
judgement per passage per search, clicking again removes it). A record holds: document SHA-256 and display name,
page count, query, retrieval mode (`dense`/`hybrid`, reported by the engine in each `results` response), ranked
results (rank, page, chunkIndex, cosine score), timestamps, extension version and, for a judged result, the page,
chunk and a ≤300-character excerpt; for a missing result, the optional expected page and note. The PDF is never
stored again.

**Export evaluation data** builds a `legalsearch-feedback/1` JSON file in the panel and hands it to Chrome's
download manager (a Blob URL; no network). **Delete** removes every `fb:` key. Nothing is ever uploaded. Records are
written only on an explicit click; ordinary searches leave no trace (the engine log still never contains queries).

## Ready for OCR (future)

The engine only receives `[{page, text}]`. Adding OCR means another producer of that same structure when `no_text`
is returned, e.g. Tesseract in the engine or an in-browser OCR. Chunking, embeddings, cache and protocol stay as they
are; only an `index_pages` message generated from images would be needed.
