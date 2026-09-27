#Requires -Version 7
<#
.SYNOPSIS
    Runs a SonarQube analysis of furet with cargo-sonar-scanner.
.DESCRIPTION
    Reads the server URL from SONAR_HOST_URL and the token from -Token,
    else from SONAR_TOKEN (user-level environment variables); the project key comes from
    [package.metadata.sonar] in Cargo.toml. Run from anywhere: the script
    moves to the repository root itself.
.PARAMETER Token
    SonarQube analysis token; overrides SONAR_TOKEN for this run only.
.PARAMETER DryRun
    Resolves and prints the configuration without contacting the server.
#>
[CmdletBinding()]
param(
    [string]$Token,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'

function Fail([string]$Message) {
    Write-Host "Run-Sonar: $Message" -ForegroundColor Red
    exit 1
}

if (-not (Get-Command cargo-sonar-scanner -ErrorAction SilentlyContinue)) {
    Fail 'cargo-sonar-scanner not found. Install it with: cargo install cargo-sonar-scanner'
}

if ([string]::IsNullOrWhiteSpace($env:SONAR_HOST_URL)) {
    Fail 'SONAR_HOST_URL is not set. Define it once at user level, then open a new terminal.'
}

if ([string]::IsNullOrWhiteSpace($Token)) {
    $Token = $env:SONAR_TOKEN
}
if ([string]::IsNullOrWhiteSpace($Token)) {
    Fail 'no token: pass -Token or set SONAR_TOKEN at user level.'
}

$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $repoRoot 'Cargo.toml'))) {
    Fail "no Cargo.toml in $repoRoot; keep this script under tools/."
}

$previousToken = $env:SONAR_TOKEN
$env:SONAR_TOKEN = $Token

Push-Location $repoRoot
try {
    $scannerArgs = @('sonar-scanner')
    if ($DryRun) { $scannerArgs += '--dry-run' }

    Write-Host "==> cargo $($scannerArgs -join ' ')  (server: $env:SONAR_HOST_URL)"
    & cargo @scannerArgs
    $code = $LASTEXITCODE
}
finally {
    Pop-Location
    $env:SONAR_TOKEN = $previousToken
}

if ($code -ne 0) { Fail "analysis failed (exit $code)." }
Write-Host 'Analysis sent. Results appear in SonarQube once the background task finishes.' -ForegroundColor Green