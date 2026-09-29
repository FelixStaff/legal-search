# Security policy

## Supported versions

| Version | Supported |
|---|---|
| 1.1.0 RC2 (current release candidate) | Yes |
| 1.1.0 RC1 | No — superseded by RC2 |
| Earlier internal builds (1.0.0) | No — please upgrade |

## Reporting a vulnerability

**Please do not report security vulnerabilities in public GitHub issues, discussions or pull requests.**

Report them privately through **GitHub Private Vulnerability Reporting** on this repository
(*Security* tab → *Report a vulnerability*). If that option is not available yet, a dedicated private security
contact will be added here before the general public release; until then, open a public issue that only says you
have a security report and asks for a private channel — without any technical details.

Useful information in a report:

- the LegalSearch version (`LegalSearch.exe --version`, or the extension version in `chrome://extensions`),
  Windows and Chrome versions;
- the affected component (Chrome extension, `LegalSearch.exe`, installer/uninstaller);
- steps to reproduce, and a minimal proof of concept if possible;
- the impact you expect (e.g. code execution, data disclosure, denial of service);
- any crafted PDF used — **only if it contains no confidential material**.

We will acknowledge the report, investigate, and coordinate a fix and disclosure timeline with you.

## Security architecture

LegalSearch has two parts: a Chrome extension (side panel) and a local engine, `LegalSearch.exe`, started by Chrome
through Native Messaging. See [docs/architecture.md](docs/architecture.md) for details.

Safeguards in the current code:

- **Native Messaging restricted to the LegalSearch extension.** The host manifest's `allowed_origins` contains only
  the LegalSearch extension ID, and the engine also refuses to run when its caller argument is not a
  `chrome-extension://` origin.
- **Strict message validation.** Closed message types (`deny_unknown_fields`), SHA-256 identifiers of exactly
  64 lowercase hex characters, bounded query (1–1000 characters), page numbers, page text size (200,000 characters),
  total text (60 M characters), result count (`k ≤ 50`) and message size (8 MB; a larger length header stops the
  process without allocating memory). Malformed messages get an error reply and are never echoed into logs.
- **No shell execution.** The engine runs no commands, shells or scripts, and never opens a file path received in a
  message: every path it uses derives from its own install location and `%LOCALAPPDATA%` (no path traversal).
  File names are display-only and sanitized.
- **PDF treated as untrusted input.** PDF.js runs in the extension with XFA and scripting disabled and never uses
  `eval`; the extension CSP is `script-src 'self'; object-src 'none'`. JavaScript embedded in a PDF is never executed.
  Extracted text is inserted into the panel with `textContent`, never as HTML.
- **Absolute DLL loading.** `onnxruntime.dll` is loaded by absolute path from the program folder, never through the
  Windows DLL search order.
- **No external API calls.** The engine opens no network connections; the extension only re-reads the PDF from its
  original address (see [PRIVACY.md](PRIVACY.md)).
- **Least privilege install.** Per-user installation under `%LOCALAPPDATA%`, no administrator rights, a single
  HKCU registry key for Native Messaging, no services, scheduled tasks, startup entries or open ports.
- **Contained failures.** Panics are caught and answered with a generic error; users never see stack traces.
- **Pinned build inputs.** The model, tokenizer and ONNX Runtime are downloaded at build time only and verified by
  SHA-256.

## Known security-relevant limitations

- The installer and `LegalSearch.exe` are **not code-signed yet**. Windows SmartScreen warns on first run, and
  Smart App Control may block unsigned binaries. Verify downloads against the published `SHA256SUMS.txt`.
- The local index cache contains document text and is protected only by Windows account permissions.
- Until Chrome Web Store publication, the extension is loaded in developer mode ("Load unpacked").

## Dependency auditing

CI runs `cargo audit` (RustSec) and `npm audit`. Current status: no known vulnerabilities. One informational advisory
is accepted: RUSTSEC-2024-0436 (`paste` unmaintained), reachable only through a test-only dev-dependency and not
part of the shipped program (see `native/.cargo/audit.toml`).
