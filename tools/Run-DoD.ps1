#requires -Version 7
# Definition-of-Done runner for furet. Runs, in order, stopping at the
# first failing step and printing which one failed:
#   1. cargo fmt --check
#   2. cargo clippy --all-targets -- -D warnings
#   3. cargo deny check
#   4. cargo test, with TMP/TEMP pointing at a throwaway dir holding a decoy
#   5. cargo build --release
#   6. gitleaks git --redact --exit-code 1 (secret scan of the full history)
#   7. the release binary with --version (its output is printed)
# Writes the full output to target/dod/dod-<timestamp>.log and prints its path first and last.
# Exits 0 only when all seven steps succeed.

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
$logDir = Join-Path $repoRoot 'target/dod'
New-Item -ItemType Directory -Force $logDir | Out-Null
$log = Join-Path $logDir ("dod-{0:yyyyMMdd-HHmmss}.log" -f (Get-Date))

function Write-Log([string] $Text) {
    Write-Host $Text
    Add-Content -Path $log -Value $Text
}

# WHY: 2>&1 plus "$_" turns cargo's stderr records into plain text, so they reach the log without tripping ErrorActionPreference.
function Invoke-Logged([string] $Command, [string[]] $Arguments) {
    & $Command @Arguments 2>&1 | ForEach-Object { "$_" } | Tee-Object -FilePath $log -Append
}

# WHY: tests create their sandboxes in the temp dir and the disk fallback walks it; a decoy matching almost any query makes that leak fail on every machine.
function Invoke-WithDecoyTemp([scriptblock] $Action) {
    $alphabet = 'abcdefghijklmnopqrstuvwxyz0123456789'
    $decoyTemp = Join-Path ([System.IO.Path]::GetTempPath()) "furet-dod-$PID"
    if (Test-Path $decoyTemp) { Remove-Item -Recurse -Force $decoyTemp }
    $decoyName = "decoy-$alphabet-$alphabet-$alphabet-$alphabet"
    New-Item -ItemType Directory -Force (Join-Path $decoyTemp $decoyName) | Out-Null
    Write-Log "Tests run with TMP/TEMP = $decoyTemp"
    $saved = $env:TMP, $env:TEMP
    try {
        $env:TMP = $decoyTemp
        $env:TEMP = $decoyTemp
        & $Action
    } finally {
        $env:TMP, $env:TEMP = $saved
        Remove-Item -Recurse -Force $decoyTemp
    }
}

$savedOutputEncoding = [Console]::OutputEncoding

try {
    # WHY: pwsh decodes piped native output with the console code page (OEM on Windows); cargo writes UTF-8.
    [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
    Write-Log "DoD log: $log"
    $steps = @(
        @{ Name = 'cargo fmt --check';     Args = @('fmt', '--check') },
        @{ Name = 'cargo clippy --all-targets -- -D warnings';
                                         Args = @('clippy', '--all-targets', '--', '-D', 'warnings') },
        @{ Name = 'cargo deny check';    Args = @('deny', 'check') },
        @{ Name = 'cargo test (decoy temp dir)'; Args = @('test'); DecoyTemp = $true },
        @{ Name = 'cargo build --release'; Args = @('build', '--release') }
    )

    $stepNumber = 0
    foreach ($step in $steps) {
        $stepNumber++
        Write-Log "==> Step $stepNumber/7: $($step.Name)"
        if ($step.DecoyTemp) {
            Invoke-WithDecoyTemp { Invoke-Logged 'cargo' $step.Args }
        } else {
            Invoke-Logged 'cargo' $step.Args
        }
        if ($LASTEXITCODE -ne 0) {
            Write-Log "FAILED at step $stepNumber/7: $($step.Name)"
            exit $LASTEXITCODE
        }
    }

    $stepNumber++
    Write-Log "==> Step $stepNumber/7: gitleaks git --redact --exit-code 1"
    $gitleaksTimer = [System.Diagnostics.Stopwatch]::StartNew()
    Invoke-Logged 'gitleaks' @('git', '--redact', '--no-banner', '--exit-code', '1')
    $gitleaksExit = $LASTEXITCODE
    Write-Log ("gitleaks step took {0:N1}s" -f $gitleaksTimer.Elapsed.TotalSeconds)
    if ($gitleaksExit -ne 0) {
        Write-Log "FAILED at step $stepNumber/7: gitleaks git"
        exit $gitleaksExit
    }

    $stepNumber++
    Write-Log "==> Step $stepNumber/7: run the release binary with --version"
    $binary = Join-Path $repoRoot 'target/release/furet'
    if ($IsWindows) { $binary += '.exe' }
    $versionOutput = & $binary '--version'
    if ($LASTEXITCODE -ne 0) {
        Write-Log "FAILED at step $stepNumber/7: running the release binary"
        exit $LASTEXITCODE
    }
    Write-Log "furet --version output:"
    Write-Log $versionOutput

    Write-Log 'All DoD steps passed.'
    exit 0
} catch {
    Write-Log "FAILED: unexpected error: $_"
    exit 1
} finally {
    [Console]::OutputEncoding = $savedOutputEncoding
    Write-Host "DoD log: $log"
    Pop-Location
}
