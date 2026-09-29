# Installation and uninstallation

## End user

### Install (once)

1. Run **`LegalSearchSetup.exe`**.
   - It does not ask for administrator rights: it installs for your user only, in `%LOCALAPPDATA%\LegalSearch`.
   - If Windows shows "Windows protected your PC" (SmartScreen, because the installer is not signed yet), click
     **More info → Run anyway**.
2. **Next → Install → Finish.** A guide opens in the browser when it finishes.
3. Follow the guide to turn on the extension in Chrome (until it is published in the Chrome Web Store):
   1. In Chrome, go to `chrome://extensions`.
   2. Turn on **Developer mode** (top right).
   3. Click **Load unpacked** and choose the folder `%LOCALAPPDATA%\LegalSearch\extension`
      (the guide shows the exact path with a **Copy** button).
   4. In LegalSearch's **Details**, turn on **Allow access to file URLs** (needed for PDFs saved on the computer).
   5. Pin the LegalSearch icon to the toolbar (puzzle piece → pin).

Once the extension is published in the Chrome Web Store, step 3 becomes a single **Add to Chrome** click: the
installer's guide switches automatically (see README, "Publish to the Chrome Web Store").

### Use

Open a PDF in Chrome → click the LegalSearch icon → the side panel opens → type your query in your own words →
**Search** → **Go to page**.

- The first time a document is opened you see "Extracting text… / Building semantic index… %" and then "Document ready".
  About 300 pages take about 7 seconds on a normal laptop.
- The next times that same PDF is opened, it is ready instantly.

### Uninstall

**Windows Settings → Apps → Installed apps → LegalSearch → Uninstall.**

The uninstaller asks:

> Also delete indexed documents and the local cache?

- **Yes**: everything is removed; nothing of LegalSearch remains on the computer.
- **No** (default): only `%LOCALAPPDATA%\LegalSearch\data\index.sqlite` is kept, so documents need not be indexed again
  after a reinstall. It can be deleted by hand at any time.

> Inno Setup does not support checkboxes in the uninstaller, so the option "[ ] Also delete indexed documents and
> local cache" is offered as a Yes/No question with "No" as the default. In a silent uninstall (`/VERYSILENT`) the
> cache **is deleted**, because it is regenerable, unless `/KEEPCACHE=1` is added.

Remove the extension in `chrome://extensions` → LegalSearch → **Remove**.

## What exactly gets installed

```
%LOCALAPPDATA%\LegalSearch\
├── bin\LegalSearch.exe              engine (5.8 MB, no dependencies beyond Windows)
├── bin\onnxruntime.dll              official Microsoft ONNX Runtime 1.28.2, CPU only
├── bin\msvcp140*.dll, vcruntime140*.dll   VC++ runtime (app-local)
├── model\model.onnx                 multilingual-e5-small INT8 (118 MB)
├── model\tokenizer.json, model_id.txt
├── extension\                       Chrome extension (for "Load unpacked")
├── licenses\                        third-party notices
├── com.legalsearch.host.json        Native Messaging host manifest (written at install)
├── GUIDE.html                       activation guide (written at install)
├── legalsearch.ico, unins000.exe/.dat
├── data\index.sqlite                index cache (created by the program)
└── logs\legalsearch.log             local technical log, rotated (created by the program)
```

Registry, **HKCU** only and without administrator rights:

- `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.legalsearch.host` → path of the manifest.
- The standard uninstall entry Windows uses for "Apps" (`HKCU\…\Uninstall\{6E0F1C2A-…}_is1`).

No services, scheduled tasks, startup entries, firewall rules or shortcuts are created, and nothing outside those
paths is modified.

Installed size: **~155 MB**. The installer is **~89 MB**.

## Verification

Automated install and uninstall (what was used to validate this version):

```powershell
# silent install
dist\LegalSearchSetup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART
powershell -File scripts\verify-install.ps1 -Expect installed     # files, registry, manifest, allowed_origins
node scripts\e2e-host.mjs --installed                             # installed engine over Native Messaging
node scripts\e2e-chrome.mjs --chrome <chrome-for-testing>\chrome.exe   # real UI in Chrome

# uninstall keeping the cache (same as answering "No")
& "$env:LOCALAPPDATA\LegalSearch\unins000.exe" /VERYSILENT /KEEPCACHE=1
powershell -File scripts\verify-install.ps1 -Expect removed -CacheKept
# after reinstalling, already indexed documents open as "ready" without re-indexing

# uninstall leaving nothing behind
& "$env:LOCALAPPDATA\LegalSearch\unins000.exe" /VERYSILENT
powershell -File scripts\verify-install.ps1 -Expect removed
```

Manual check that nothing remains:

1. `Settings → Apps`: LegalSearch no longer appears.
2. The folder `%LOCALAPPDATA%\LegalSearch` does not exist (or only contains `data\` if you chose to keep the cache).
3. `reg query HKCU\Software\Google\Chrome\NativeMessagingHosts\com.legalsearch.host` reports it cannot be found.
4. Task Manager shows no `LegalSearch.exe`.
5. In Chrome, the LegalSearch panel (if the extension is still installed) shows "The LegalSearch local engine is not installed."

## Development install (without the installer)

Register the built engine by hand (useful while developing):

```powershell
$exe = "$PWD\native\target\release\LegalSearch.exe"
$id  = (Get-Content config\extension.config.json | ConvertFrom-Json).extensionId
@{ name='com.legalsearch.host'; description='LegalSearch local engine'; path=$exe; type='stdio';
   allowed_origins=@("chrome-extension://$id/") } | ConvertTo-Json | Set-Content -Encoding utf8 native\target\host.json
reg add HKCU\Software\Google\Chrome\NativeMessagingHosts\com.legalsearch.host /ve /d "$PWD\native\target\host.json" /f
```

In development the exe looks for the model in `..\model` next to `bin\`. Set `LEGALSEARCH_MODEL_DIR` (folder
`models\current`) and `LEGALSEARCH_ORT_DLL` (`native\runtime\onnxruntime.dll`) as user environment variables, or use
the installer. `LEGALSEARCH_HYBRID=1` turns on the experimental hybrid (dense + BM25) retrieval.
