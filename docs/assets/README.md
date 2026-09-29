# Screenshots

Real screenshots of LegalSearch, produced by the Chrome UI end-to-end test (never edited or mocked):

```powershell
$env:SCREENSHOTS_DIR = "$PWD\docs\assets"
node scripts\e2e-chrome.mjs --chrome <Chrome for Testing>\chrome.exe
```

The test document is the fictitious Spanish ruling in `tests/fixtures/corpus.json`.
