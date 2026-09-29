# Reproducible build of every deliverable:
#   dist\LegalSearchSetup.exe        installer (engine + model + runtime + extension)
#   dist\extension\                  unpacked extension (development "Load unpacked")
#   dist\legalsearch-extension-store.zip   package for the Chrome Web Store
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-all.ps1 [-SkipTests]
#
# Build-machine prerequisites (never needed by end users):
#   Rust (rustup, MSVC toolchain), Visual Studio Build Tools (C++), Node.js >= 20,
#   Inno Setup 6. Internet access only to fetch the pinned model/runtime once.
param([switch]$SkipTests)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Step($m) { Write-Host "`n==> $m" -ForegroundColor Cyan }
function Run($exe, [string[]]$argv) {
    & $exe @argv
    if ($LASTEXITCODE -ne 0) { throw "$exe $($argv -join ' ') failed ($LASTEXITCODE)" }
}

# ---------------------------------------------------------------- prerequisites
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
foreach ($c in 'cargo', 'node', 'npm') {
    if (-not (Get-Command $c -ErrorAction SilentlyContinue)) { throw "$c not found (see README, 'Build requirements')" }
}
$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 not found (winget install JRSoftware.InnoSetup)' }

# ---------------------------------------------------------------- inputs
Step 'Model and runtime (pinned, SHA-256 verified)'
Run powershell @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "$root\scripts\fetch-model.ps1")
Run powershell @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "$root\scripts\fetch-runtime.ps1")
Run node @("$root\scripts\gen-dev-key.mjs")
Run node @("$root\scripts\make-fixture.mjs")
Run node @("$root\scripts\make-icons.mjs")

# ---------------------------------------------------------------- engine
Step 'Engine (Rust)'
Push-Location native
try {
    if (-not $SkipTests) {
        # Windows Smart App Control (when enforced) may refuse to start a freshly
        # built, unsigned test binary ("os error 4551"). That is not a test failure:
        # rerun the suite with the debug profile (same code, different binary).
        # Via cmd: in Windows PowerShell 5.1, redirecting a native command's stderr
        # under ErrorActionPreference=Stop turns cargo's progress lines into errors.
        $out = & cmd.exe /d /c 'cargo test --release --locked 2>&1'
        $code = $LASTEXITCODE
        $out | Write-Host
        if ($code -ne 0) {
            if ($out -match 'os error 4551') {
                Write-Host 'Smart App Control blocked a release test binary; rerunning tests in debug profile.' -ForegroundColor Yellow
                Run cargo @('test', '--locked')
            } else {
                throw 'cargo test failed'
            }
        }
    }
    Run cargo @('build', '--release', '--locked')
} finally { Pop-Location }

# ---------------------------------------------------------------- extension
Step 'Extension'
Push-Location extension
try {
    Run npm @('ci', '--no-audit', '--no-fund')
    if (-not $SkipTests) { Run npm @('test') }
    Run node @('build.mjs', '--store')
    $zip = Join-Path $root 'dist\legalsearch-extension-store.zip'
    New-Item -ItemType Directory -Force (Split-Path $zip) | Out-Null
    if (Test-Path $zip) { Remove-Item $zip }
    Compress-Archive -Path 'build\*' -DestinationPath $zip
    Run node @('build.mjs')   # development build (stable ID) is what ships in the installer
} finally { Pop-Location }

if (-not $SkipTests) {
    Step 'Engine end-to-end (Native Messaging protocol)'
    Run node @("$root\scripts\e2e-host.mjs", '--dev')
}

# ---------------------------------------------------------------- stage + installer
Step 'Installer'
$stage = Join-Path $root 'dist\stage'
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force "$stage\bin", "$stage\model", "$stage\extension", "$stage\licenses" | Out-Null
Copy-Item native\target\release\LegalSearch.exe "$stage\bin\"
Copy-Item native\runtime\*.dll "$stage\bin\"
Copy-Item models\current\model.onnx, models\current\tokenizer.json, models\current\model_id.txt "$stage\model\"
Copy-Item -Recurse extension\build\* "$stage\extension\"
Copy-Item native\runtime\LICENSE-onnxruntime.txt, native\runtime\ThirdPartyNotices-onnxruntime.txt "$stage\licenses\"
Copy-Item LICENSE "$stage\licenses\LICENSE-LegalSearch.txt"
Copy-Item THIRD_PARTY_NOTICES.md "$stage\licenses\"
Copy-Item installer\licenses\* "$stage\licenses\"
Copy-Item extension\build\LICENSE-pdfjs.txt "$stage\licenses\"
Run node @("$root\scripts\gen-rust-licenses.mjs", "$stage\licenses\THIRD_PARTY_RUST_CRATES.txt")

Run $iscc @('/Qp', "$root\installer\LegalSearch.iss")

$ext = Join-Path $root 'dist\extension'
if (Test-Path $ext) { Remove-Item -Recurse -Force $ext }
Copy-Item -Recurse extension\build $ext
Copy-Item docs\tester-guide.md (Join-Path $root 'dist\TESTER-GUIDE.md') -Force
Remove-Item -Recurse -Force $stage

Step 'Checksums'
# sha256sum-compatible format: "<hex>  <file>" (verify with: Get-FileHash <file>, or sha256sum -c SHA256SUMS.txt)
$sums = foreach ($f in 'LegalSearchSetup.exe', 'legalsearch-extension-store.zip') {
    '{0}  {1}' -f (Get-FileHash (Join-Path 'dist' $f) -Algorithm SHA256).Hash.ToLower(), $f
}
[IO.File]::WriteAllText((Join-Path $root 'dist\SHA256SUMS.txt'), (($sums -join "`n") + "`n"))
Get-Content dist\SHA256SUMS.txt

Step 'Done'
Get-ChildItem dist | Format-Table Name, Length
