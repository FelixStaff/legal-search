# Changelog

All notable changes to LegalSearch are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.1.0-rc2] - 2026-09-28

Second release candidate. No retrieval, ranking or user-interface behavior changes compared with RC1.

### Added
- Repository documentation for public release: `LICENSE` (MIT), `PRIVACY.md`, `SECURITY.md`,
  `THIRD_PARTY_NOTICES.md`, `CONTRIBUTING.md`, issue and pull request templates.
- GitHub Actions: CI (formatting, build, Rust and extension tests, engine end-to-end test, dependency audits) and a
  tag-triggered release workflow that builds the installer, extension package and `SHA256SUMS.txt`.
- The installer ships the license texts of every bundled third-party component, including all linked Rust crates.
- Real screenshots in `docs/assets/`, captured by the UI end-to-end test (`SCREENSHOTS_DIR`).
- `scripts/check-version.mjs` (version consistency) and `dist/SHA256SUMS.txt` in the build output.

- Release documentation: `docs/release-checklist.md`, `docs/code-signing.md`, `docs/chrome-web-store.md`,
  `docs/github-setup.md` and draft release notes in `docs/releases/`.

### Changed
- Version `1.1.0-rc2` (installer `AppVersion`, extension `version_name` "1.1.0 RC2"; numeric version stays `1.1.0`).
- Rust code formatted with `rustfmt` (`native/rustfmt.toml`); CI enforces it.
- CI uses current major versions of the official GitHub actions and Node.js 24 LTS.

### Fixed
- The engine log could, for a malformed message, include part of that message in a JSON error description.
  JSON errors are now logged as category and position only.

## [1.1.0-rc1] - 2026-09-28

First release candidate for testing with lawyers.

### Added
- Optional local relevance feedback: **Relevant** / **Not relevant** on each result and **Missing result?** (with an
  optional expected page and note). Stored only in the Chrome profile; nothing is uploaded.
- **Export evaluation data**: JSON export (`legalsearch-feedback/1`) with document hash, query, retrieval mode, ranked
  results, judged passage and judgement, for offline retrieval benchmarking. **Delete** removes all feedback.
- Experimental hybrid retrieval (dense + BM25 with weighted Reciprocal Rank Fusion). **Disabled by default**; opt-in
  with `LEGALSEARCH_HYBRID=1`. Evaluation results are in `docs/architecture.md`.
- The engine reports the retrieval mode used (`dense` / `hybrid`) with each search result.
- Tester guide (`docs/tester-guide.md`).

### Changed
- All product-facing text (side panel, installer, activation guide, documentation) is now in English.
- The activation guide is now `GUIDE.html`.
- The uninstaller removes its own temporary copy from `%TEMP%` after finishing.
- On-disk cache format versioning (`PRAGMA user_version`); caches from an incompatible version are rebuilt instead
  of being read.
- The Chrome extension uses the PDF.js legacy build, so it runs on Chrome 116 and later.

- Silent uninstalls can keep the cache with `/KEEPCACHE=1`.

### Security
- BM25 and fusion failures are contained: search falls back to dense-only and the error is logged.

## [1.0.0] - 2026-09-28

Internal, unreleased build (never distributed publicly).

### Added
- Chrome extension (Manifest V3, side panel) that extracts PDF text per page with PDF.js, for web and local PDFs.
- Local engine `LegalSearch.exe` (Rust): page-aware chunking, multilingual-e5-small INT8 embeddings via ONNX Runtime
  (CPU), exact cosine Top-K search, SQLite cache keyed by the PDF's SHA-256, Chrome Native Messaging.
- Compact SentencePiece tokenizer (about 6 MB instead of about 234 MB), verified identical to Hugging Face `tokenizers`.
- "Go to page" navigation (`#page=N` + reload).
- Per-user Inno Setup installer and uninstaller with an optional cache-deletion prompt.
- Detection of scanned PDFs without a text layer.

[Unreleased]: https://github.com/OWNER/legal-search/compare/v1.1.0-rc2...HEAD
[1.1.0-rc2]: https://github.com/OWNER/legal-search/releases/tag/v1.1.0-rc2
[1.1.0-rc1]: https://github.com/OWNER/legal-search/releases/tag/v1.1.0-rc1
