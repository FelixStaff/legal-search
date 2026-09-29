# Chrome Web Store preparation

Status: **not submitted**. The extension currently has to be loaded manually ("Load unpacked"). This page collects
what is needed to publish it.

## Package to upload

`dist/legalsearch-extension-store.zip`, produced by `scripts/build-all.ps1` (or `cd extension; node build.mjs --store`).

- Manifest V3, `version` `1.1.0`, `version_name` `1.1.0 RC2`.
- **No `key` field** (the store assigns the extension ID and signs the package).
- Contents: side panel page, service worker, bundled PDF.js (legacy build) and its worker, character maps, icons,
  `LICENSE.txt`, `LICENSE-pdfjs.txt`. No remotely hosted code, no `eval`.

## Permissions (audited for RC2)

| Permission | Why it is needed | Can it be narrower? |
|---|---|---|
| `sidePanel` | The whole UI lives in Chrome's side panel next to the PDF. | No. |
| `nativeMessaging` | The only channel to the local engine `LegalSearch.exe` (indexing and search run there). | No. |
| `tabs` | Read the URL of the active tab to identify the PDF (including `file://` URLs, so the panel can explain the missing file-access permission), follow tab switches, and set `#page=N` + reload for "Go to page". | Not without losing the file-access guidance; kept. |
| `storage` | Optional local relevance feedback (`chrome.storage.local`); nothing is synced or uploaded. | No. |
| Host `http://*/*`, `https://*/*` | Re-read the bytes of the PDF the user is already viewing, from its own URL (any site can serve a PDF). | Yes, in principle — see note. |
| Host `file:///*` | Read local PDFs (Chrome additionally requires the user to enable "Allow access to file URLs"). | No. |

No permission was added for RC2. **Note for review:** broad host permissions trigger the "Read and change all your
data on all websites" warning and a more detailed store review. A future version could request host access
per site at runtime (`optional_host_permissions`) or rely on `activeTab`; both change the user flow (an extra
permission prompt, or losing automatic re-detection when switching tabs) and are deliberately not part of RC2.

## Privacy disclosures (dashboard "Privacy practices" tab)

- **Single purpose**: "Semantic search inside the PDF open in the current tab, processed locally on the user's
  computer."
- **Data handling** (to be confirmed against the dashboard's current wording by the owner):
  - PDF text (website content) is read and processed **locally** by the local engine; it is not transmitted to the
    developer or any third party.
  - Search queries are not transmitted. They are stored locally only if the user gives optional feedback.
  - No personal information, authentication data, location, browsing history or analytics are collected or sent.
  - Data is not sold, not used for unrelated purposes, and not used for creditworthiness or lending.
- **Remote code**: none.
- **Privacy policy URL**: the published `PRIVACY.md` of the repository (e.g.
  `https://github.com/OWNER/legal-search/blob/main/PRIVACY.md`).

## Listing text

**Name**: LegalSearch

**Short description** (max. 132 characters):
> Local semantic search for legal PDFs. Find passages by meaning and jump to the page — the document never leaves your computer.

**Detailed description**:

> LegalSearch adds a side panel to Chrome's PDF viewer. Describe what you are looking for in your own words and it
> shows the passages of the open document that mean that — even when the document uses different wording — with
> their page number and a "Go to page" button.
>
> Everything runs on your computer: text extraction happens in the browser and semantic search runs in the LegalSearch
> desktop engine, installed separately with LegalSearchSetup.exe (Windows). No cloud services, no AI APIs, no accounts,
> no telemetry.
>
> Requirements: Windows 10/11 x64 and the LegalSearch desktop engine. Scanned PDFs without a text layer need OCR first.
> LegalSearch is a document retrieval tool; it does not provide legal advice.

**Category**: Productivity (Tools).

## Assets needed

| Asset | Requirement (check the dashboard for current rules) | Status |
|---|---|---|
| Store icon | 128×128 PNG (the extension's `icons/icon128.png`; Google recommends 96×96 artwork with transparent padding) | Existing icon; review padding |
| Screenshots | At least 1, up to 5; 1280×800 or 640×400 | **Missing** — `docs/assets/` has real screenshots but not at store dimensions |
| Small promo tile | 440×280 PNG | **Missing** |
| Marquee promo tile | 1400×560 PNG (optional) | Missing (optional) |

Screenshots must show the real product; they can be captured with `SCREENSHOTS_DIR` in
`scripts/e2e-chrome.mjs` and a browser window sized to 1280×800.

## Steps that require the owner's Chrome Web Store account

1. Register a Chrome Web Store developer account (Google charges a one-time registration fee) and verify the
   publisher email.
2. Create the item and upload `legalsearch-extension-store.zip`.
3. Fill in the listing, assets, category, language and the Privacy practices tab (permission justifications above).
4. Choose visibility (e.g. *Unlisted* for the pilot) and submit for review.
5. After the item is created, copy its **extension ID** and **public key** from the dashboard into
   `config/extension.config.json` (`extensionId`, `manifestKey`, `webStoreUrl`) and rebuild with
   `scripts/build-all.ps1`, so the installer's Native Messaging host allows the store extension ID and the setup
   guide links to the store instead of explaining "Load unpacked".

## Current verdict

```text
CHROME_WEB_STORE = NEEDS_WORK
```

The package itself is ready to upload. Store-size screenshots, the promo tile, the developer account and the
dashboard disclosures are still to do, and the broad host permissions should be reviewed with the store's current
policy in mind.
