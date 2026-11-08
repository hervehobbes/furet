# Entry point of the commit-msg hook: checks the subject line of the commit message file given as argument.
# Exit code 0 when the subject is valid, 1 when it is rejected or the file cannot be read.
param([Parameter(Mandatory)][string]$Path)

Import-Module (Join-Path $PSScriptRoot 'CommitMessage.psm1') -Force

try {
    $message = Get-Content -LiteralPath $Path -Raw -Encoding utf8 -ErrorAction Stop
} catch {
    [Console]::Error.WriteLine("commit-msg: cannot read '$Path': $($_.Exception.Message)")
    exit 1
}

$reason = Test-CommitSubject $message
if ($null -eq $reason) {
    exit 0
}

$help = @(
    "commit-msg: commit rejected, $reason.",
    'Expected format: type(scope): subject',
    'Types: feat, fix, perf, refactor, chore, build, test, docs, style, ci, revert',
    'Example: fix(storage): keep the query memory after a restart'
)
foreach ($line in $help) {
    [Console]::Error.WriteLine($line)
}
exit 1
