# Releases a new X or Y version: generates CHANGELOG.md with git-cliff, commits it and tags the commit.
# Run by hand only, on main, on a clean tree. Never run by the DoD or a hook.
# The push command is printed, not executed.
#
# Exit code 0 on success, 1 on any failed step.
$ErrorActionPreference = 'Continue'

$repoRoot = Split-Path $PSScriptRoot -Parent
$cargoToml = Join-Path $repoRoot 'Cargo.toml'

Write-Host "0. Checking that git-cliff is installed."
if (-not (Get-Command git-cliff -ErrorAction SilentlyContinue)) {
    Write-Error "git-cliff not found in PATH. Install it with: winget install git-cliff"
    exit 1
}

Write-Host "1. Checking the working tree and the branch."
$dirty = @(git -C $repoRoot status --porcelain)
if ($LASTEXITCODE -ne 0) {
    Write-Error "git status failed (exit $LASTEXITCODE)."
    exit 1
}
if ($dirty.Count -gt 0) {
    Write-Error "Working tree is not clean ($($dirty.Count) entry(ies)). Commit or discard the changes first."
    exit 1
}
$branch = git -C $repoRoot rev-parse --abbrev-ref HEAD
if ($LASTEXITCODE -ne 0) {
    Write-Error "git rev-parse failed (exit $LASTEXITCODE)."
    exit 1
}
if ($branch -ne 'main') {
    Write-Error "Current branch is '$branch', expected 'main'."
    exit 1
}

Write-Host "2. Reading the version from $cargoToml."
$content = Get-Content -LiteralPath $cargoToml -Raw
if ($content -notmatch '(?m)^version = "(\d+\.\d+\.\d+)"') {
    Write-Error "No version = ""X.Y.Z"" line found in $cargoToml."
    exit 1
}
$version = $Matches[1]
Write-Host "   Version: $version"

git -C $repoRoot rev-parse -q --verify "refs/tags/v$version" *> $null
if ($LASTEXITCODE -eq 0) {
    Write-Error "Tag v$version already exists."
    exit 1
}
if ($LASTEXITCODE -gt 1) {
    Write-Error "git rev-parse failed while looking for tag v$version (exit $LASTEXITCODE)."
    exit 1
}

Write-Host "3. Prepending the v$version section to CHANGELOG.md with git-cliff."
Push-Location $repoRoot
git-cliff --unreleased --tag "v$version" --prepend CHANGELOG.md
$code = $LASTEXITCODE
Pop-Location
if ($code -ne 0) {
    Write-Error "git-cliff failed (exit $code)."
    exit 1
}

Write-Host "4. Committing CHANGELOG.md only."
git -C $repoRoot add CHANGELOG.md
if ($LASTEXITCODE -ne 0) {
    Write-Error "git add failed (exit $LASTEXITCODE)."
    exit 1
}
git -C $repoRoot commit -m "chore(release): v$version"
if ($LASTEXITCODE -ne 0) {
    Write-Error "git commit failed (exit $LASTEXITCODE)."
    exit 1
}

Write-Host "5. Tagging v$version."
git -C $repoRoot tag -a "v$version" -m "Release v$version"
if ($LASTEXITCODE -ne 0) {
    Write-Error "git tag failed (exit $LASTEXITCODE)."
    exit 1
}

Write-Host "6. Done. To publish, run: git push origin main --follow-tags"
exit 0
