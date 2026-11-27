# Windows counterpart of scripts/dev.sh -- the same CI checks and dev entry points.
#
#   scripts\dev.ps1 check    fmt + clippy (same as CI)
#   scripts\dev.ps1 test     all workspace tests (same as CI)
#   scripts\dev.ps1 ui       frontend typecheck + i18n parity + build (same as CI)
#   scripts\dev.ps1 dev      run the desktop app against the Vite dev server (hot reload)
#   scripts\dev.ps1 web      run only the frontend in a browser (no Rust backend)
#   scripts\dev.ps1 cli      build and run the CLI against an isolated data dir
#   scripts\dev.ps1 bundle   produce a signed-capable bundle (needs tauri-cli)
#
# Prefer the cmd wrapper if you do not want to deal with the execution policy:
#   scripts\dev.cmd dev
[CmdletBinding()]
param(
  [Parameter(Position = 0)]
  [string]$Command = 'check',

  [Parameter(Position = 1, ValueFromRemainingArguments = $true)]
  [string[]]$Rest = @()
)

$ErrorActionPreference = 'Stop'

# cd "$(dirname "$0")/.."
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $RepoRoot

if ([string]::IsNullOrEmpty($env:CARGO_HOME)) {
  $env:CARGO_HOME = Join-Path $RepoRoot '.cargo-home'
}
# Keep the toolchain stable even when a developer runs with a custom profile dir.
if ([string]::IsNullOrEmpty($env:RUSTUP_HOME)) {
  $userHome = if (-not [string]::IsNullOrEmpty($env:USERPROFILE)) { $env:USERPROFILE } else { $env:HOME }
  if ($userHome) {
    $rustup = Join-Path $userHome '.rustup'
    if (Test-Path -LiteralPath $rustup -PathType Container) { $env:RUSTUP_HOME = $rustup }
  }
}

# Replaces `set -e`: run a native command and abort with its exit code.
function Invoke-Checked {
  param(
    [Parameter(Mandatory = $true, Position = 0)][string]$File,
    [Parameter(Position = 1)][string[]]$Arguments = @()
  )
  if (-not (Get-Command $File -CommandType Application -ErrorAction SilentlyContinue) -and
      -not (Test-Path -LiteralPath $File)) {
    Write-Host "error: '$File' not found on PATH." -ForegroundColor Red
    exit 127
  }
  & $File @Arguments
  if ($LASTEXITCODE -ne 0) {
    Write-Host ("error: '{0} {1}' failed (exit {2})" -f $File, ($Arguments -join ' '), $LASTEXITCODE) -ForegroundColor Red
    exit $LASTEXITCODE
  }
}

function Test-TauriCli {
  $previous = $ErrorActionPreference
  $ErrorActionPreference = 'SilentlyContinue'
  try {
    & cargo tauri --version | Out-Null
    return ($LASTEXITCODE -eq 0)
  } catch {
    return $false
  } finally {
    $ErrorActionPreference = $previous
  }
}

# Vite runs with strictPort on a fixed port (ui/vite.config.ts) and Tauri's devUrl
# points at it (src-tauri/tauri.conf.json), so a leftover dev server makes
# `tauri dev` fail with "Port 5183 is already in use". Fail early with a useful
# message instead, and offer to reap a stray node process on request.
$DevPort = 5183

# netstat works on every Windows build without elevation, unlike some CIM paths.
function Get-DevPortPids {
  $previous = $ErrorActionPreference
  $ErrorActionPreference = 'SilentlyContinue'
  try {
    $lines = & netstat -ano
    $pids = foreach ($line in $lines) {
      if ($line -notmatch 'LISTENING') { continue }
      $cols = ($line.Trim() -split '\s+')
      if ($cols.Count -lt 5) { continue }
      if ($cols[1] -match (':{0}$' -f $DevPort) -and $cols[-1] -match '^\d+$') { $cols[-1] }
    }
    return @($pids | Sort-Object -Unique)
  } catch {
    return @()
  } finally {
    $ErrorActionPreference = $previous
  }
}

function Assert-DevPort {
  $pids = Get-DevPortPids
  if ($pids.Count -eq 0) { return }
  Write-Host ("error: port {0} is already in use by PID(s): {1}" -f $DevPort, ($pids -join ' ')) -ForegroundColor Red
  & netstat -ano | Select-String -Pattern (':{0}\s' -f $DevPort) | ForEach-Object { Write-Host ("  " + $_.Line.Trim()) }

  if ($env:KILL_STALE_DEV -eq '1') {
    Write-Host 'KILL_STALE_DEV=1: stopping the process(es) above.'
    foreach ($p in $pids) {
      # 0 (Idle) and 4 (System) can never own a Vite port; never try to kill them.
      if ([int]$p -le 4) { continue }
      Stop-Process -Id ([int]$p) -Force -ErrorAction SilentlyContinue
    }
    for ($i = 0; $i -lt 10; $i++) {
      if ((Get-DevPortPids).Count -eq 0) { break }
      Start-Sleep -Milliseconds 500
    }
    $still = Get-DevPortPids
    if ($still.Count -gt 0) {
      Write-Host ("error: port {0} still busy ({1}); stop those processes manually." -f $DevPort, ($still -join ' ')) -ForegroundColor Red
      exit 1
    }
    return
  }

  Write-Host ''
  Write-Host 'A previous dev server probably survived. Either stop it:'
  Write-Host ("  Stop-Process -Id {0} -Force" -f ($pids -join ', '))
  Write-Host 'or re-run with KILL_STALE_DEV=1 to have this script do it:'
  Write-Host '  set KILL_STALE_DEV=1 && scripts\dev.cmd dev'
  exit 1
}

switch ($Command.ToLowerInvariant()) {
  'check' {
    Invoke-Checked cargo @('fmt', '--all', '--', '--check')
    Invoke-Checked cargo @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings')
  }
  'test' {
    Invoke-Checked cargo @('test', '--workspace', '--all-targets')
  }
  'ui' {
    Invoke-Checked pnpm @('--dir', 'ui', 'typecheck')
    Invoke-Checked pnpm @('--dir', 'ui', 'i18n:check')
    Invoke-Checked pnpm @('--dir', 'ui', 'ipc:check')
    Invoke-Checked pnpm @('--dir', 'ui', 'build')
  }
  'dev' {
    # Tauri dev: Vite on :5183 with hot reload, Rust rebuilt on change.
    Assert-DevPort
    if (Test-TauriCli) {
      Invoke-Checked cargo @('tauri', 'dev')
      exit 0
    }
    Write-Host 'tauri-cli not installed; falling back to the manual two-process flow:' -ForegroundColor Yellow
    Write-Host '  terminal 1: pnpm --dir ui dev'
    Write-Host '  terminal 2: cargo run -p heron-app'
    Write-Host ''
    Write-Host "cargo install tauri-cli --version '^2' --locked   # to enable hot-reloading backend"
    exit 1
  }
  'web' {
    Invoke-Checked pnpm @('--dir', 'ui', 'dev')
  }
  'cli' {
    Invoke-Checked cargo @('build', '-p', 'heron-cli')
    # HERON_DEV_DATA wins, then %TEMP%, then an in-repo scratch dir.
    $dataDir = if ($env:HERON_DEV_DATA) {
      $env:HERON_DEV_DATA
    } elseif ($env:TEMP) {
      Join-Path $env:TEMP 'heron-dev'
    } else {
      Join-Path $RepoRoot 'target\heron-dev'
    }
    $exe = Join-Path $RepoRoot 'target\debug\heron.exe'
    if (-not (Test-Path -LiteralPath $exe)) {
      Write-Host "error: $exe not found (cargo build produced nothing?)" -ForegroundColor Red
      exit 1
    }
    $arguments = @('--data-dir', $dataDir)
    if ($Rest) { $arguments += $Rest }
    Invoke-Checked $exe $arguments
  }
  'bundle' {
    if (-not (Test-TauriCli)) {
      Write-Host "bundle needs tauri-cli: cargo install tauri-cli --version '^2' --locked" -ForegroundColor Red
      exit 1
    }
    Invoke-Checked pnpm @('--dir', 'ui', 'build')
    Invoke-Checked cargo @('tauri', 'build')
  }
  default {
    Write-Host "usage: $PSCommandPath [check|test|ui|dev|web|cli|bundle]" -ForegroundColor Red
    exit 2
  }
}
