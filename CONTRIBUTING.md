# Contributing to LegalSearch

Thanks for your interest. LegalSearch handles confidential legal documents, so two principles come first:
**everything stays on the user's computer**, and **embeddings remain the primary retrieval signal**.
Changes that add network calls, telemetry, accounts or cloud services will not be accepted.

## Prerequisites (Windows)

| Tool | Install |
|---|---|
| Rust (MSVC toolchain) | `winget install Rustlang.Rustup` |
| Visual Studio Build Tools with the C++ workload | `winget install Microsoft.VisualStudio.2022.BuildTools` |
| Node.js ≥ 20 | `winget install OpenJS.NodeJS.LTS` |
| Inno Setup 6 (installer only) | `winget install JRSoftware.InnoSetup` |

## Repository layout

```
config/      extension ID and Native Messaging host name (single source of truth)
extension/   Chrome extension (Manifest V3, TypeScript, PDF.js) and its tests
native/      LegalSearch.exe (Rust): protocol, chunking, tokenizer, embeddings, search, cache, tests
installer/   Inno Setup script, activation guide, bundled license texts
scripts/     build, model/runtime download, fixtures, end-to-end tests, install verification
tests/       fixtures (generated test PDFs, Spanish legal evaluation corpora)
docs/        architecture, installation, troubleshooting, tester guide
```

## Development build

```powershell
scripts\fetch-model.ps1            # model + tokenizer → models\current (build time only)
scripts\fetch-runtime.ps1          # ONNX Runtime + VC++ runtime → native\runtime
node scripts\make-fixture.mjs      # test PDFs
cd native;    cargo build --release
cd extension; npm ci; npm run build   # load extension\build with "Load unpacked"
```

Full reproducible build of all artifacts: `scripts\build-all.ps1` (see README).

## Tests

| Suite | Command |
|---|---|
| Rust formatting | `cd native; cargo fmt --check` |
| Rust unit + model tests | `cd native; cargo test --release` (model tests are skipped if the model is not fetched) |
| Retrieval ablation report | `cargo test --release --test retrieval_eval -- --nocapture` |
| Extension | `cd extension; npm test` (after `node scripts/make-fixture.mjs`) |
| Engine end-to-end (Native Messaging protocol) | `node scripts/e2e-host.mjs --dev` |
| Chrome UI end-to-end | `node scripts/e2e-chrome.mjs --chrome <Chrome for Testing>\chrome.exe` (engine must be installed) |
| Install / uninstall | see [docs/installation.md](docs/installation.md#verification) |

CI runs formatting, build, Rust and extension tests, the engine end-to-end test and dependency audits on every push
and pull request. The Chrome UI and install/uninstall tests are part of local release validation.

## Coding expectations

- Match the style of the surrounding code; keep comments short and useful. Rust code is formatted with `cargo fmt`.
- Never log document text or queries. Never add network access to the engine.
- Treat PDF content and Native Messaging messages as untrusted input; validate and bound everything.
- Changes to chunking, pooling or the model must bump `PIPELINE_VERSION` (`native/src/embed.rs`); incompatible cache
  table changes must bump `SCHEMA_VERSION` (`native/src/cache.rs`).
- Ranking changes need evidence: run `retrieval_eval` and report the before/after metrics in the pull request.
- Product-facing text is English. Test fixtures that model Spanish legal documents stay in Spanish.
- Do not commit PDFs, models, binaries, feedback exports, logs, certificates or secrets (see `.gitignore`).

## Pull requests

Keep them focused, fill in the pull request template (including privacy, security and installation impact) and make
sure CI passes.

## Security vulnerabilities

Do **not** open public issues for vulnerabilities. Follow [SECURITY.md](SECURITY.md).
