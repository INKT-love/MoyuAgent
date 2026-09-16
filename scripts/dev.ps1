$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectRoot
$portableEnvironment = Join-Path $projectRoot '.tools/env-msvc.ps1'
if (Test-Path -LiteralPath $portableEnvironment) { . $portableEnvironment }
else { $env:PATH = "$env:USERPROFILE/.cargo/bin;$env:PATH" }
$runningServer = Get-NetTCPConnection -LocalPort 1420 -State Listen -ErrorAction SilentlyContinue
if ($runningServer) {
    npm run tauri -- dev --config src-tauri/tauri.existing-server.json
} else {
    npm run tauri -- dev
}
exit $LASTEXITCODE
