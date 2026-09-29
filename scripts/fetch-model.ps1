# Downloads the embedding model at BUILD time (never at end-user install time).
# Files are pinned to a Hugging Face commit-independent LFS SHA-256 and verified.
#
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-model.ps1
#
param([ValidateSet('small','base')][string]$Model = 'small')
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$root = Split-Path -Parent $PSScriptRoot
# The installer always packages models\current. 'small' is the default
# (see docs/architecture.md, "Model choice").
$dest = Join-Path $root 'models\current'
New-Item -ItemType Directory -Force $dest | Out-Null

$catalog = @{
    small = @{
        Repo = 'Xenova/multilingual-e5-small'; Id = 'intfloat/multilingual-e5-small@int8';
        Model = 'f80102d3f2a1229f387d3c81909990d8945513e347b0eab049f7de3c6f98c193';
        Tokenizer = '0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39' };
    base = @{
        Repo = 'Xenova/multilingual-e5-base'; Id = 'intfloat/multilingual-e5-base@int8';
        Model = 'df7a9a29309e3ad491e1783adf8baee710262cc06079c7cbab63c630277fac94';
        Tokenizer = '62c24cdc13d4c9952d63718d6c9fa4c287974249e16b7ade6d5a85e7bbb75626' }
}
$m = $catalog[$Model]
$base = "https://huggingface.co/$($m.Repo)/resolve/main"
$files = @(
    @{ Remote = 'onnx/model_quantized.onnx'; Local = 'model.onnx'; Sha256 = $m.Model },
    @{ Remote = 'tokenizer.json'; Local = 'tokenizer.json'; Sha256 = $m.Tokenizer }
)
# Switching model: wipe the previous one.
$idFile = Join-Path $dest 'model_id.txt'
if ((Test-Path $idFile) -and ((Get-Content $idFile -Raw).Trim() -ne $m.Id)) {
    Remove-Item (Join-Path $dest '*') -Force
}

foreach ($f in $files) {
    $out = Join-Path $dest $f.Local
    if (Test-Path $out) {
        $h = (Get-FileHash -Algorithm SHA256 $out).Hash.ToLower()
        if ($h -eq $f.Sha256) { Write-Host "OK (cached) $($f.Local)"; continue }
        Remove-Item $out
    }
    Write-Host "Downloading $($f.Remote) ..."
    Invoke-WebRequest -Uri "$base/$($f.Remote)" -OutFile "$out.part" -UseBasicParsing
    $h = (Get-FileHash -Algorithm SHA256 "$out.part").Hash.ToLower()
    if ($h -ne $f.Sha256) {
        Remove-Item "$out.part"
        throw "SHA-256 mismatch for $($f.Remote): got $h"
    }
    Move-Item "$out.part" $out
    Write-Host "OK $($f.Local)"
}
Set-Content -Path $idFile -Value $m.Id -Encoding ascii -NoNewline
Write-Host "Model ready in $dest ($($m.Id))"
