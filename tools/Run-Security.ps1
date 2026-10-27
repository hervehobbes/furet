#requires -Version 7
# Security scan for furet, run by hand: not part of the DoD, since it needs the network and pulls the Grype database.
# Runs every step even when an earlier one fails, then names the failing blocking steps:
#   1. grype db update        (blocking)
#   2. grype db status        (blocking)
#   3. trivy fs, full report  (report only)
#   4. trivy fs, gate         (blocking: any HIGH/CRITICAL secret, misconfiguration, or vulnerability with a fix)
#   5. grype dir, full report (report only)
#   6. grype dir, gate        (blocking: any vulnerability rated high or above with a fix)
#   7. cargo update --dry-run --verbose (report only: outdated crates; Cargo.lock is not written)
# Writes the full output to target/security/security-<timestamp>.log and prints its path first and last.
# Exits 0 only when every blocking step succeeds.

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
$logDir = Join-Path $repoRoot 'target/security'
New-Item -ItemType Directory -Force $logDir | Out-Null
$log = Join-Path $logDir ("security-{0:yyyyMMdd-HHmmss}.log" -f (Get-Date))

function Write-Log([string] $Text) {
    Write-Host $Text
    Add-Content -Path $log -Value $Text
}

# WHY: 2>&1 plus "$_" turns the scanners' stderr records into plain text, so they reach the log without tripping ErrorActionPreference.
function Invoke-Logged([string] $Command, [string[]] $Arguments) {
    & $Command @Arguments 2>&1 | ForEach-Object { "$_" } | Tee-Object -FilePath $log -Append
}

$trivyFs = @('fs', '.', '--skip-dirs', 'target', '--skip-dirs', 'prompts', '--skip-dirs', 'mutants.out*',
             '--skip-dirs', '.sonar', '--scanners', 'vuln,secret,misconfig')
$grypeDir = @('dir:.', '--exclude', './.git/**', '--exclude', './target/**', '--exclude', './prompts/**',
              '--exclude', './mutants.out*/**', '--exclude', './.sonar/**')

$steps = @(
    @{ Name = 'grype db update';    Command = 'grype'; Args = @('db', 'update'); Blocking = $true },
    @{ Name = 'grype db status';    Command = 'grype'; Args = @('db', 'status'); Blocking = $true },
    @{ Name = 'trivy fs (report)';  Command = 'trivy'; Args = $trivyFs; Blocking = $false },
    @{ Name = 'trivy fs (gate)';    Command = 'trivy';
       Args = $trivyFs + @('--severity', 'HIGH,CRITICAL', '--ignore-unfixed', '--exit-code', '1'); Blocking = $true },
    @{ Name = 'grype dir (report)'; Command = 'grype'; Args = $grypeDir + @('-o', 'table'); Blocking = $false },
    @{ Name = 'grype dir (gate)';   Command = 'grype';
       Args = $grypeDir + @('--only-fixed', '--fail-on', 'high'); Blocking = $true },
    @{ Name = 'outdated crates (report)'; Command = 'cargo';
       Args = @('update', '--dry-run', '--verbose'); Blocking = $false }
)

$savedOutputEncoding = [Console]::OutputEncoding
$failed = @()

try {
    # WHY: pwsh decodes piped native output with the console code page (OEM on Windows); the scanners write UTF-8.
    [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
    Write-Log "Security log: $log"
    $stepNumber = 0
    foreach ($step in $steps) {
        $stepNumber++
        Write-Log "==> Step $stepNumber/$($steps.Count): $($step.Name)"
        if ($null -eq (Get-Command $step.Command -ErrorAction SilentlyContinue)) {
            Write-Log "Tool not found on PATH: $($step.Command)"
            if ($step.Blocking) { $failed += $step.Name }
            continue
        }
        Invoke-Logged $step.Command $step.Args
        if ($LASTEXITCODE -ne 0) {
            Write-Log "Exit code $LASTEXITCODE from step $stepNumber/$($steps.Count): $($step.Name)"
            if ($step.Blocking) { $failed += $step.Name }
        }
    }

    if ($failed.Count -eq 0) {
        Write-Log 'All blocking security steps passed.'
        exit 0
    }
    Write-Log "FAILED blocking steps: $($failed -join ', ')"
    exit 1
} catch {
    Write-Log "FAILED: unexpected error: $_"
    exit 1
} finally {
    [Console]::OutputEncoding = $savedOutputEncoding
    Write-Host "Security log: $log"
    Pop-Location
}
