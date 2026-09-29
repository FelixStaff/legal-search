# Prepares native\runtime\ at BUILD time with:
#   - onnxruntime.dll: official Microsoft ONNX Runtime CPU build (pinned + SHA-256 verified)
#   - Visual C++ runtime DLLs (app-local deployment, redistributable per the VS license)
# These are packaged by the installer next to LegalSearch.exe. End users download nothing.
#
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-runtime.ps1
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$OrtVersion = '1.28.2'
$OrtZipSha256 = 'c4eedd29489d5feca21866d054638416f3655bf6b18851b3b6b85c8313e95c35'

$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root 'native\runtime'
New-Item -ItemType Directory -Force $dest | Out-Null

# ---- ONNX Runtime
$dll = Join-Path $dest 'onnxruntime.dll'
$stamp = Join-Path $dest 'onnxruntime.version'
if ((Test-Path $dll) -and (Test-Path $stamp) -and ((Get-Content $stamp -Raw).Trim() -eq $OrtVersion)) {
    Write-Host "OK (cached) onnxruntime $OrtVersion"
} else {
    $name = "onnxruntime-win-x64-$OrtVersion"
    $tmp = Join-Path ([IO.Path]::GetTempPath()) "legalsearch-ort-$([guid]::NewGuid())"
    New-Item -ItemType Directory $tmp | Out-Null
    try {
        $zip = Join-Path $tmp "$name.zip"
        Write-Host "Downloading ONNX Runtime $OrtVersion ..."
        Invoke-WebRequest "https://github.com/microsoft/onnxruntime/releases/download/v$OrtVersion/$name.zip" -OutFile $zip -UseBasicParsing
        $h = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
        if ($h -ne $OrtZipSha256) { throw "SHA-256 mismatch for ONNX Runtime zip: $h" }
        Expand-Archive $zip -DestinationPath $tmp
        Copy-Item (Join-Path $tmp "$name\lib\onnxruntime.dll") $dll -Force
        Copy-Item (Join-Path $tmp "$name\LICENSE") (Join-Path $dest 'LICENSE-onnxruntime.txt') -Force
        Copy-Item (Join-Path $tmp "$name\ThirdPartyNotices.txt") (Join-Path $dest 'ThirdPartyNotices-onnxruntime.txt') -Force
        Set-Content $stamp $OrtVersion -Encoding ascii -NoNewline
        Write-Host "OK onnxruntime $OrtVersion"
    } finally {
        Remove-Item -Recurse -Force $tmp
    }
}

# ---- Visual C++ runtime (needed by onnxruntime.dll; LegalSearch.exe itself uses a static CRT)
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vswhere)) { throw 'vswhere.exe not found: install Visual Studio Build Tools (C++).' }
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Visual C++ build tools not found.' }
$crt = Get-ChildItem "$vs\VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT" -Directory | Sort-Object FullName | Select-Object -Last 1
if (-not $crt) { throw 'VC++ redistributable folder not found.' }
foreach ($f in 'msvcp140.dll', 'msvcp140_1.dll', 'vcruntime140.dll', 'vcruntime140_1.dll') {
    Copy-Item (Join-Path $crt.FullName $f) (Join-Path $dest $f) -Force
}
Write-Host "OK VC++ runtime from $($crt.FullName)"
