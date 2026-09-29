# Privacy

LegalSearch is designed for confidential legal documents. This page describes exactly what it stores and what it
sends, based on the current source code (version 1.1.0 RC2).

## Summary

- PDFs are processed **locally**, on your computer. Their contents are not uploaded anywhere.
- Search queries are **not uploaded** anywhere.
- Embeddings (the numeric representation used for semantic search) are **generated locally** on your CPU by a model
  installed with the program.
- There is **no telemetry**, **no analytics**, **no crash reporting**, **no external AI API** and **no user account**.
- Optional relevance feedback is stored **only in your Chrome profile** and leaves it only if **you** click
  **Export evaluation data** and then share the resulting file yourself.

## Data sent over the network

**By the LegalSearch engine (`LegalSearch.exe`): none.** It communicates only with Chrome, on the same computer,
through Chrome Native Messaging (standard input/output of a local process). It opens no network connections and
no listening ports.

**By the LegalSearch Chrome extension:** only one kind of request, and only for PDFs opened from a website:

- To read the PDF you are viewing, the extension requests it again **from the same address** Chrome already
  loaded it from (`http://` or `https://`), with the same cookies, allowing the browser cache to answer. This request
  goes to the website that serves the PDF, exactly like reloading the page. Nothing is sent to LegalSearch, its
  authors or any third party.
- For local files (`file://`) no network request is made at all.

**By the installer:** none. Everything it installs is inside `LegalSearchSetup.exe`.

**By Chrome itself** (not LegalSearch): if the extension is later installed from the Chrome Web Store, Chrome checks
the store for extension updates as it does for any extension.

**When building from source** (developers only): the build scripts download the pinned model and ONNX Runtime files
from Hugging Face and GitHub once. End users never do this.

## Data stored locally

| What | Where | Contents | How to remove |
|---|---|---|---|
| Index cache | `%LOCALAPPDATA%\LegalSearch\data\index.sqlite` | For each indexed PDF: its SHA-256 hash, file name, page count, indexing time and, for every passage, the page number, the **passage text** and its embedding. Together the passages contain essentially the **full text** of the document. Up to the 300 most recently used documents. | Uninstall and answer "Yes" to deleting the cache, or delete the file |
| Technical log | `%LOCALAPPDATA%\LegalSearch\logs\legalsearch.log` (+ `.1.log`), rotated at 512 KB | Events, timings, counters and error descriptions (which can include local file paths). **Never** document text or search queries. | Deleted on uninstall |
| Evaluation feedback (optional) | Chrome profile, `chrome.storage.local` of the LegalSearch extension | Only when you click **Relevant**, **Not relevant** or **Missing result?**: the PDF's SHA-256 hash and file name, page count, **your query**, the retrieval mode, the ranked results (page, passage number, score), the judged passage's page and an **excerpt of up to 300 characters**, your judgement, optional expected page and note, timestamps and the extension version. The PDF itself is never stored. | **Delete** button in the panel, or remove the extension |
| Exported evaluation file (optional) | Wherever Chrome saves downloads (`legalsearch-evaluation-<date>.json`) | A copy of the feedback records above. | Delete the file |

Ordinary searches leave no record: queries are kept only inside feedback you choose to give.

The index cache is protected only by your Windows account permissions, like any other file in your user profile.
If your documents require it, use full-disk encryption (for example BitLocker).

## Sharing exported evaluation data

An exported evaluation file **can contain your queries and short excerpts of your documents**. Review it before
sharing it, share it only through a channel you trust, and do not attach it to public GitHub issues if the
documents are confidential.

## Contact

For privacy questions, open an issue in the project repository **without** including confidential material.
For security vulnerabilities, follow [SECURITY.md](SECURITY.md).
