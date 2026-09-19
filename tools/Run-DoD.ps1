#requires -Version 7
# Definition-of-Done runner for furet. Runs, in order, stopping at the
# first failing step and printing which one failed:
#   1. cargo fmt --check
#   2. cargo clippy --all-targets -- -D warnings
#   3. cargo test
#   4. cargo build --release
#   5. the release binary with --version (its output is printed)
# Exits 0 only when all five steps succeed.

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    $steps = @(
        @{ Name = 'cargo fmt --check';     Args = @('fmt', '--check') },
        @{ Name = 'cargo clippy --all-targets -- -D warnings';
                                         Args = @('clippy', '--all-targets', '--', '-D', 'warnings') },
        @{ Name = 'cargo test';            Args = @('test') },
        @{ Name = 'cargo build --release'; Args = @('build', '--release') }
    )

    $stepNumber = 0
    foreach ($step in $steps) {
        $stepNumber++
        Write-Host "==> Step $stepNumber/5: $($step.Name)"
        & cargo @($step.Args)
        if ($LASTEXITCODE -ne 0) {
            Write-Host "FAILED at step $stepNumber/5: $($step.Name)"
            exit $LASTEXITCODE
        }
    }

    $stepNumber++
    Write-Host "==> Step $stepNumber/5: run the release binary with --version"
    $binary = Join-Path $repoRoot 'target/release/furet'
    if ($IsWindows) { $binary += '.exe' }
    $versionOutput = & $binary '--version'
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED at step $stepNumber/5: running the release binary"
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
