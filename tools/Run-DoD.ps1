#requires -Version 7
# Definition-of-Done runner for furet. Runs, in order, stopping at the
# first failing step and printing which one failed:
#   1. cargo fmt --check
#   2. cargo clippy --all-targets -- -D warnings
#   3. cargo deny check
#   4. cargo test
#   5. cargo build --release
#   6. gitleaks git --redact --exit-code 1 (secret scan of the full history)
#   7. the release binary with --version (its output is printed)
# Exits 0 only when all seven steps succeed.

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    $steps = @(
        @{ Name = 'cargo fmt --check';     Args = @('fmt', '--check') },
        @{ Name = 'cargo clippy --all-targets -- -D warnings';
                                         Args = @('clippy', '--all-targets', '--', '-D', 'warnings') },
        @{ Name = 'cargo deny check';    Args = @('deny', 'check') },
        @{ Name = 'cargo test';          Args = @('test') },
        @{ Name = 'cargo build --release'; Args = @('build', '--release') }
    )

    $stepNumber = 0
    foreach ($step in $steps) {
        $stepNumber++
        Write-Host "==> Step $stepNumber/7: $($step.Name)"
        & cargo @($step.Args)
        if ($LASTEXITCODE -ne 0) {
            Write-Host "FAILED at step $stepNumber/7: $($step.Name)"
            exit $LASTEXITCODE
        }
    }

    $stepNumber++
    Write-Host "==> Step $stepNumber/7: gitleaks git --redact --exit-code 1"
    $gitleaksTimer = [System.Diagnostics.Stopwatch]::StartNew()
    & gitleaks git --redact --no-banner --exit-code 1
    $gitleaksExit = $LASTEXITCODE
    Write-Host ("gitleaks step took {0:N1}s" -f $gitleaksTimer.Elapsed.TotalSeconds)
    if ($gitleaksExit -ne 0) {
        Write-Host "FAILED at step $stepNumber/7: gitleaks git"
        exit $gitleaksExit
    }

    $stepNumber++
    Write-Host "==> Step $stepNumber/7: run the release binary with --version"
    $binary = Join-Path $repoRoot 'target/release/furet'
    if ($IsWindows) { $binary += '.exe' }
    $versionOutput = & $binary '--version'
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED at step $stepNumber/7: running the release binary"
        exit $LASTEXITCODE
    }
    Write-Host "furet --version output:"
    Write-Host $versionOutput

    Write-Host 'All DoD steps passed.'
    exit 0
} catch {
    Write-Host "FAILED: unexpected error: $_"
    exit 1
} finally {
    Pop-Location
}
