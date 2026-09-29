# Release checklist

Copy this list into the release issue or pull request for every version. Status below is for **1.1.0-rc2**,
validated locally on 2026-09-28 (Windows 11, Chrome for Testing 154 for UI tests).

## Validation

- [x] All tests pass — `scripts/build-all.ps1` (Rust unit + real-model tests, extension tests, engine end-to-end)
      and `scripts/e2e-chrome.mjs`
- [x] Version consistency verified — `node scripts/check-version.mjs v1.1.0-rc2`
- [x] Clean installation tested — `scripts/verify-install.ps1 -Expect installed` after install from a clean state
- [x] Web PDF tested — `e2e-chrome.mjs` (http PDF: indexing, search, Go to page)
- [x] Local PDF tested — `e2e-chrome.mjs` (file:// PDF: search, Go to page)
- [x] Cache reuse tested — reopen in Chrome, and reinstall after uninstall with `/KEEPCACHE=1`
- [x] Feedback export tested — `e2e-chrome.mjs` (Relevant / Not relevant / Missing result, export file, Delete)
- [x] Uninstall with cache deletion tested — `verify-install.ps1 -Expect removed -Since <start>`
- [x] Uninstall preserving cache tested — `verify-install.ps1 -Expect removed -CacheKept`
- [x] SHA-256 checksums generated — `dist/SHA256SUMS.txt`, verified with `sha256sum -c`
- [x] Dependency audits — `cargo audit`, `npm audit`

## Review

- [x] THIRD_PARTY_NOTICES reviewed
- [x] PRIVACY.md reviewed
- [x] SECURITY.md reviewed
- [x] Release notes prepared — `docs/releases/1.1.0-rc2.md` (draft)

## Not done yet

- [ ] Installer signed — see `docs/code-signing.md` (`CODE_SIGNING = NOT_CONFIGURED`)
- [ ] Chrome Web Store package validated — the package builds, but it has not been uploaded or reviewed by the store
      (`docs/chrome-web-store.md`)
- [ ] GitHub Actions CI passed on GitHub — workflows have not run yet (repository not created)
- [ ] Interactive uninstall answering "No" to the cache question tested by hand (the automated test covers the same
      code path with `/KEEPCACHE=1`)
- [ ] Tested in regular (branded) Google Chrome by a person, including the "Allow access to file URLs" guidance
- [ ] Private vulnerability reporting enabled on the GitHub repository

## Commands

```powershell
node scripts\check-version.mjs v<version>
powershell -ExecutionPolicy Bypass -File scripts\build-all.ps1          # tests + artifacts + SHA256SUMS.txt
cd dist; sha256sum -c SHA256SUMS.txt                                     # Git Bash, or Get-FileHash
# On a machine WITHOUT an existing LegalSearch installation:
powershell -File scripts\verify-install.ps1 -Expect removed              # must pass before continuing
dist\LegalSearchSetup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART
powershell -File scripts\verify-install.ps1 -Expect installed
node scripts\e2e-host.mjs --installed
node scripts\e2e-chrome.mjs --chrome <Chrome for Testing>\chrome.exe
& "$env:LOCALAPPDATA\LegalSearch\unins000.exe" /VERYSILENT /KEEPCACHE=1
powershell -File scripts\verify-install.ps1 -Expect removed -CacheKept
dist\LegalSearchSetup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART      # cache reused
& "$env:LOCALAPPDATA\LegalSearch\unins000.exe" /VERYSILENT
powershell -File scripts\verify-install.ps1 -Expect removed -Since <start time>
```
