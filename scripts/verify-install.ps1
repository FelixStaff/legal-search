# Verifies the per-user installation state of LegalSearch.
#   powershell -ExecutionPolicy Bypass -File scripts\verify-install.ps1 -Expect installed
#   powershell -ExecutionPolicy Bypass -File scripts\verify-install.ps1 -Expect removed [-CacheKept]
param(
    [Parameter(Mandatory)][ValidateSet('installed', 'removed')][string]$Expect,
    [switch]$CacheKept,
    # Also fail on installer/uninstaller temp folders (%TEMP%\is-*) created after this time.
    [string]$Since = ''
)
$ErrorActionPreference = 'Stop'
$app = Join-Path $env:LOCALAPPDATA 'LegalSearch'
$hostKey = 'HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.legalsearch.host'
$fail = 0
function Check($ok, $what) {
    if ($ok) { Write-Host "  PASS  $what" } else { Write-Host "  FAIL  $what" -ForegroundColor Red; $script:fail++ }
}
$uninstall = Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' -ErrorAction SilentlyContinue |
    Where-Object { (Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue).DisplayName -eq 'LegalSearch' }
$proc = Get-Process LegalSearch -ErrorAction SilentlyContinue

if ($Expect -eq 'installed') {
    foreach ($f in 'bin\LegalSearch.exe', 'bin\onnxruntime.dll', 'bin\vcruntime140.dll', 'model\model.onnx',
                   'model\tokenizer.json', 'model\model_id.txt', 'extension\manifest.json', 'com.legalsearch.host.json',
                   'GUIDE.html', 'unins000.exe', 'licenses\LICENSE-LegalSearch.txt', 'licenses\THIRD_PARTY_NOTICES.md',
                   'licenses\THIRD_PARTY_RUST_CRATES.txt', 'licenses\LICENSE-onnxruntime.txt',
                   'licenses\ThirdPartyNotices-onnxruntime.txt', 'licenses\LICENSE-pdfjs.txt',
                   'licenses\multilingual-e5-small-LICENSE.txt') {
        Check (Test-Path (Join-Path $app $f)) "file $f"
    }
    Check (Test-Path $hostKey) 'Native Messaging registry key (HKCU)'
    $manifestPath = (Get-ItemProperty $hostKey -ErrorAction SilentlyContinue).'(default)'
    Check ($manifestPath -eq (Join-Path $app 'com.legalsearch.host.json')) "registry points to host manifest"
    $m = Get-Content (Join-Path $app 'com.legalsearch.host.json') -Raw | ConvertFrom-Json
    Check ($m.path -eq (Join-Path $app 'bin\LegalSearch.exe')) 'host manifest path is absolute and correct'
    Check ($m.type -eq 'stdio') 'host manifest type stdio'
    $cfg = Get-Content (Join-Path (Split-Path -Parent $PSScriptRoot) 'config\extension.config.json') -Raw | ConvertFrom-Json
    Check (($m.allowed_origins.Count -eq 1) -and ($m.allowed_origins[0] -eq "chrome-extension://$($cfg.extensionId)/")) 'allowed_origins = only the LegalSearch extension'
    Check ($null -ne $uninstall) 'Windows "Apps" uninstall entry'
    $size = (Get-ChildItem $app -Recurse -File | Measure-Object Length -Sum).Sum / 1MB
    Write-Host ("        installed size: {0:N1} MB" -f $size)
} else {
    Check (-not (Test-Path $hostKey)) 'Native Messaging registry key removed'
    Check ($null -eq $uninstall) 'uninstall entry removed'
    Check ($null -eq $proc) 'no LegalSearch process running'
    if ($CacheKept) {
        $left = @(Get-ChildItem $app -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.FullName -notlike "$app\data\*" })
        Check ($left.Count -eq 0) "only the cache remains (kept on request): $($left.FullName -join ', ')"
    } else {
        Check (-not (Test-Path $app)) "application folder removed ($app)"
    }
    $tasks = @(Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object { $_.TaskName -like '*LegalSearch*' })
    Check ($tasks.Count -eq 0) 'no scheduled tasks'
    $svc = @(Get-Service -ErrorAction SilentlyContinue | Where-Object { $_.Name -like '*LegalSearch*' })
    Check ($svc.Count -eq 0) 'no services'
    $run = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -ErrorAction SilentlyContinue
    Check (-not ($run.PSObject.Properties.Name -match 'LegalSearch')) 'no startup entries'
}
# Both states: no installer/uninstaller leftovers.
$setupProcs = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -like '_unins*' -or $_.ProcessName -like 'unins0*' -or $_.ProcessName -like 'LegalSearchSetup*' -or $_.ProcessName -like 'is-*' })
Check ($setupProcs.Count -eq 0) "no installer/uninstaller process running $($setupProcs.ProcessName -join ', ')"
if ($Since) {
    $t = [datetime]::Parse($Since)
    $temps = @(Get-ChildItem $env:TEMP -Directory -Filter 'is-*' -ErrorAction SilentlyContinue | Where-Object { $_.CreationTime -ge $t })
    Check ($temps.Count -eq 0) "no temporary installation folders left in %TEMP% $($temps.Name -join ', ')"
}
$other = @(Get-ChildItem 'HKCU:\Software' -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -like '*LegalSearch*' })
Check ($other.Count -eq 0) 'no other LegalSearch registry keys under HKCU\Software'

if ($fail) { Write-Host "$fail check(s) FAILED" -ForegroundColor Red; exit 1 } else { Write-Host 'ALL CHECKS PASSED' }
