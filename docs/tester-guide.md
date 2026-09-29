# LegalSearch 1.1.0 RC2 — guide for test users

Thank you for testing LegalSearch. This release candidate is meant to find the searches where it **fails**, so
please use it on your real documents and report both the good and the bad results.

## 1. Install

1. Run `LegalSearchSetup.exe` → **Next** → **Install** → **Finish**.
   If Windows says "Windows protected your PC", click **More info → Run anyway** (the program is not signed yet).
2. Follow the guide that opens to turn on the extension in Chrome (about one minute, only once).

## 2. Use it as you normally would

Open a PDF in Chrome, click the LegalSearch icon and type what you are looking for, in your own words.
The first time a document is opened it needs a few seconds to prepare.

## 3. Tell us how each search went (optional, stays on your computer)

Under every result there are two buttons:

- **Relevant**: this passage answers what you searched for.
- **Not relevant**: it does not.

Click the same button again to undo. If the passage you expected is not in the list, click **Missing result?**
below the results and, if you know it, type the page where it actually is (a short note is welcome too).

Everything you mark is saved **only on this computer**, inside Chrome. Nothing is sent anywhere.

## 4. Send us the results

At the bottom of the panel, click **Export evaluation data (N)**. Chrome saves a file named
`legalsearch-evaluation-<date>.json` in your Downloads folder.

The file contains, for each judged search: your query, the list of pages returned, the page and a short excerpt
(up to 300 characters) of the passage you judged, your judgement, the time, and a fingerprint (SHA-256) of the
document — not the document itself. **Review it before sharing, since queries and excerpts may be confidential.**
Share it only through a channel you trust.

To also let us reproduce a failure, we will need the PDF itself, shared separately if you are allowed to.

**Delete** (next to the export button) removes all saved feedback from this computer. Removing the extension also
removes it.

## 5. Uninstall

Windows Settings → Apps → LegalSearch → Uninstall. Remove the extension in `chrome://extensions` → Remove.
