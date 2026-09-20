$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectRoot

# Prefer the bundled Node runtime when it is not already available on PATH.
$nodeCandidates = @(
    (Join-Path $projectRoot '.tools/node/node.exe'),
    (Join-Path $env:USERPROFILE '.codex/tools/node-v22.14.0-win-x64/node.exe')
)
$nodePath = $nodeCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if ($nodePath) {
    $nodeDir = Split-Path -Parent $nodePath
    $env:PATH = "$nodeDir;$env:PATH"
}

# Load the Visual Studio C++ toolchain required by Tauri's Rust build.
$portableEnvironment = Join-Path $projectRoot '.tools/env-msvc.ps1'
if (Test-Path -LiteralPath $portableEnvironment) { . $portableEnvironment }
else {
    $vsDevCmdCandidates = @(
        'C:\Program Files\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat',
        'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat'
    )
    $vsDevCmd = $vsDevCmdCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $vsDevCmd) { throw 'Visual Studio Build Tools not found. Install the C++ build tools workload.' }

    $vsEnvironment = cmd.exe /d /c "call `"$vsDevCmd`" -arch=x64 && set"
    foreach ($line in $vsEnvironment) {
        if ($line -match '^(?<name>[^=]+)=(?<value>.*)$') {
            Set-Item -Path "Env:$($Matches.name)" -Value $Matches.value
        }
    }
    $env:PATH = "$env:USERPROFILE/.cargo/bin;$env:PATH"
}

$npmCommand = Get-Command npm.cmd -ErrorAction SilentlyContinue
if (-not $npmCommand) { throw 'npm.cmd not found. Install Node.js or configure the bundled Node runtime.' }

$runningServer = Get-NetTCPConnection -LocalPort 1420 -State Listen -ErrorAction SilentlyContinue
if ($runningServer) {
    & $npmCommand.Source run tauri -- dev --config src-tauri/tauri.existing-server.json
} else {
    & $npmCommand.Source run tauri -- dev
}
exit $LASTEXITCODE
