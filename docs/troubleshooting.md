# Troubleshooting

## Panel messages

| Message | Cause | What to do |
|---|---|---|
| **The LegalSearch local engine is not installed.** | Chrome cannot find the Native Messaging host: LegalSearch is not installed, was uninstalled, or the extension ID does not match `allowed_origins`. | Run `LegalSearchSetup.exe` and click **Retry**. If you use an extension loaded from another folder or with another ID, see "Extension ID" below. |
| **Open a PDF in this tab to search it.** | The tab URL does not end in `.pdf`. | If the tab really is a PDF (portals that serve PDFs without an extension), click **This tab is a PDF: try anyway**. |
| **This tab cannot be analyzed.** | Chrome internal pages (`chrome://`), the Web Store, etc. | Open the PDF from a website or from your computer. |
| **To search files on your computer, Chrome needs a permission.** | "Allow access to file URLs" is off for the extension. | Click **Open settings** → turn the option on → go back to the PDF. |
| **This PDF contains no extractable text.** / **…appears to be a scanned document…** | The PDF is made of images only. | OCR is needed and this version does not include it. Run the PDF through an OCR tool (e.g. Acrobat's "Recognize Text") and open it again. |
| **This PDF is password-protected and cannot be read.** | PDF encrypted with an open password. | Save a copy without a password. |
| **This PDF could not be read.** | The PDF could not be downloaded again (expired session, one-time link), the file is damaged or it is not a PDF. | Reload the tab, or download the PDF and open it from your computer. Click **Retry**. |
| **The document is being indexed.** | A search was made before preparation finished. | Wait for "Document ready". |
| **Something went wrong. Close and reopen the panel.** | Unexpected error; the details are in the log. | Close and reopen the panel. If it happens again, check the log. |

## "Go to page" reloads the PDF

This is intentional. Chrome's PDF viewer only applies `#page=N` when the document opens, so LegalSearch sets the page
in the URL and reloads the tab. The search and the index are kept. Side effects: the zoom level is lost and, for PDFs
produced by a form submission (POST), Chrome may ask to confirm resubmission.

## Technical log

`%LOCALAPPDATA%\LegalSearch\logs\legalsearch.log` (and `legalsearch.1.log`), rotated at 512 KB. It only contains
events, timings and errors, never document text or queries. Example:

```
1790631455 [INFO] pid=49000 open: cache hit (494 chunks)
1790631455 [INFO] pid=49000 model loaded in 312 ms
1790631455 [INFO] pid=49000 search: 494 chunks, dense 2112 us, bm25 0 us (0 hits), fusion 3 us, total 2 ms
```

Common log errors:

| Line | Meaning |
|---|---|
| `model load failed: onnxruntime.dll not found at …` | Incomplete installation: reinstall. |
| `model load failed: model files missing in …` | `model\model.onnx` or `tokenizer.json` is missing: reinstall. |
| `refusing caller with unexpected origin argument` | Someone ran `LegalSearch.exe` outside Chrome; ignored. |
| `cannot open cache, using memory` | `data\` is not writable; search works but without cache. |
| `cache schema vN != v1: rebuilding cache` | The cache was written by an incompatible version; it was cleared and documents are re-indexed on open. |
| `bm25 build failed; continuing dense-only` / `bm25 search failed…` | Only with hybrid retrieval enabled: the lexical signal failed; results are the dense ones. |
| `read: message too large` | Out-of-bounds message; the process exits for safety. |

## Check the engine registration

```powershell
reg query HKCU\Software\Google\Chrome\NativeMessagingHosts\com.legalsearch.host
Get-Content "$env:LOCALAPPDATA\LegalSearch\com.legalsearch.host.json"
```

`path` must point to `…\LegalSearch\bin\LegalSearch.exe` and `allowed_origins` must contain exactly
`chrome-extension://<extension ID>/`. The ID is shown in `chrome://extensions` with developer mode on.

## Extension ID

The ID is set by `config/extension.config.json`: the public key `manifestKey` determines the development ID. If you
load an extension built without `key` (e.g. the `--store` build), Chrome assigns it another ID and the engine rejects
it ("not installed" message). Fix: load `dist\extension` (it carries `key`) or update `extensionId` in the
configuration and rebuild the installer.

## Windows blocks the program

- **SmartScreen** ("Windows protected your PC") when running the installer: *More info → Run anyway*.
- **Smart App Control** (Windows 11) can block unsigned executables without reputation. If the panel says "engine not
  installed" even though it is installed, check *Windows Security → App & browser control*. The definitive fix is
  signing the installer and `LegalSearch.exe` with a code-signing certificate.

## Performance

- First indexing: ~2 s per 100 pages on a modern laptop CPU (4 threads).
- RAM: ~5 MB with the panel open and no document; ~165 MB with a document ready (model loaded); ~250 MB peak while
  indexing. The process exits and frees everything when the panel closes.
- If the computer is short on memory, close the panel when not in use.

## Reset the cache

Close the LegalSearch panels and delete `%LOCALAPPDATA%\LegalSearch\data\index.sqlite`. Documents will be indexed
again when opened.
