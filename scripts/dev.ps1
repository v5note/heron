# Windows counterpart of scripts/dev.sh -- the same CI checks and dev entry points.
#
#   scripts\dev.ps1 check    fmt + clippy (same as CI)
#   scripts\dev.ps1 test     all workspace tests (same as CI)
#   scripts\dev.ps1 ui       frontend typecheck + i18n parity + build (same as CI)
#   scripts\dev.ps1 dev      run the desktop app against the Vite dev server (hot reload;
#                            without tauri-cli it starts Vite + `cargo run -p heron-app` itself)
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

# cargo resolves `cargo <subcommand>` through PATH alone, and ADR-0001 lets CARGO_HOME
# be the in-repo .cargo-home, whose bin\ is usually not on PATH. Without this a
# perfectly installed tauri-cli answers "error: no such command: `tauri`".
$CargoBin = Join-Path $env:CARGO_HOME 'bin'
$PathDirs = @($env:PATH -split ';' | Where-Object { $_ })
if ((Test-Path -LiteralPath $CargoBin -PathType Container) -and ($PathDirs -notcontains $CargoBin)) {
  $env:PATH = (@($CargoBin) + $PathDirs) -join ';'
}

function Test-OnPath {
  param([Parameter(Mandatory = $true)][string]$Name)
  $previous = $ErrorActionPreference
  $ErrorActionPreference = 'SilentlyContinue'
  try { return [bool](Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue) }
  finally { $ErrorActionPreference = $previous }
}

function Test-TauriCli {
  # HERON_NO_TAURI=1 exercises the fallback on a machine that does have tauri-cli.
  if ($env:HERON_NO_TAURI -eq '1') { return $false }
  $previous = $ErrorActionPreference
  $ErrorActionPreference = 'SilentlyContinue'
  try {
    # `cargo: no such command` is cargo's own stderr; silence it or it reads as a
    # failure of `dev` rather than of the optional tauri-cli probe.
    & cargo tauri --version *> $null
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

# `tauri dev` does two things for us: keep Vite on :5183 and rebuild/launch the shell.
# The debug binary already reads devUrl from tauri.conf.json, so running both by hand
# costs only the Rust-side hot reload (re-run `dev` after editing Rust).
function Stop-DevTree {
  param($Proc)
  if (-not $Proc) { return }
  $previous = $ErrorActionPreference
  $ErrorActionPreference = 'SilentlyContinue'
  try {
    # pnpm.cmd spawns node.exe as a child; Stop-Process would leave it holding :5183.
    # Only ever signal the process we started, and only while it is still ours.
    if (-not $Proc.HasExited) {
      & taskkill /PID $Proc.Id /T /F /nologo | Out-Null
    }
  } finally {
    $ErrorActionPreference = $previous
  }
}

function Start-ManualDev {
  if (-not (Test-OnPath 'cargo')) {
    Write-Host "error: 'cargo' not found on PATH." -ForegroundColor Red
    return 127
  }
  # Get-Command returns *every* PATH match, and a Windows install commonly has two
  # pnpm.CMD shims (corepack dir + a global prefix). Passing that array straight to
  # -FilePath fails with "cannot convert System.Object[] to System.String".
  $pnpmPath = $null
  foreach ($cmd in @(Get-Command pnpm -CommandType Application -ErrorAction SilentlyContinue)) {
    foreach ($prop in 'Source', 'Path') {
      $candidate = [string]$cmd.$prop
      if ($candidate) { $pnpmPath = $candidate; break }
    }
    if ($pnpmPath) { break }
  }
  if (-not $pnpmPath) {
    Write-Host "error: 'pnpm' not found on PATH (install it with: corepack enable)." -ForegroundColor Red
    return 127
  }
  Write-Host 'tauri-cli not installed: starting Vite and cargo run -p heron-app directly.' -ForegroundColor Yellow
  Write-Host '  the backend will not hot-reload; re-run this command after editing Rust.' -ForegroundColor Yellow
  Write-Host "  cargo install tauri-cli --version '^2' --locked   # to get the full tauri dev flow" -ForegroundColor Yellow
  Write-Host ''

  $vite = $null
  try {
    $startVite = @{
      FilePath         = $pnpmPath
      ArgumentList     = @('--dir', 'ui', 'dev')
      WorkingDirectory = $RepoRoot
      NoNewWindow      = $true
      PassThru         = $true
    }
    try {
      $vite = Start-Process @startVite
    } catch {
      Write-Host "error: cannot start the Vite server via '$pnpmPath'." -ForegroundColor Red
      Write-Host "  $($_.Exception.Message)" -ForegroundColor Red
      return 1
    }
    # A cold Vite start pre-bundles dependencies, which is slow on Windows: 2 minutes.
    $ready = $false
    for ($i = 0; $i -lt 240; $i++) {
      if ((Get-DevPortPids).Count -gt 0) { $ready = $true; break }
      if ($vite.HasExited) { break }
      Start-Sleep -Milliseconds 500
    }
    if (-not $ready) {
      Write-Host ("error: the Vite dev server never listened on :{0}; see its output above." -f $DevPort) -ForegroundColor Red
      return 1
    }
    Write-Host ("dev server ready on http://localhost:{0}; building heron-app..." -f $DevPort)
    & cargo run -p heron-app
    return $LASTEXITCODE
  } finally {
    Stop-DevTree $vite
  }
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
    exit (Start-ManualDev)
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
