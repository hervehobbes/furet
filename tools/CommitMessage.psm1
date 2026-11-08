# Validation of commit message subjects (Conventional Commits) for the furet commit-msg hook.

# Returns $null when the first line of the message is valid, else the reason for rejection.
function Test-CommitSubject {
    param([string]$Message)

    if ([string]::IsNullOrWhiteSpace($Message)) {
        return 'the message is empty'
    }

    $firstLine = ($Message -split "`n", 2)[0].TrimEnd([char]13)

    # Git-generated subjects are exempt: a merge or revert may legitimately name a branch containing a rejected word.
    if ($firstLine -cmatch '^(Merge |Revert "|fixup! )') {
        return $null
    }

    if ($firstLine -match '\b(lot|prompt)\b') {
        return "the subject contains 'lot' or 'prompt' as a whole word"
    }

    if ($firstLine.EndsWith('.')) {
        return 'the subject ends with a period'
    }

    if ($firstLine -cnotmatch '^(feat|fix|perf|refactor|chore|build|test|docs|style|ci|revert)(\([a-z0-9-]+\))?!?: \S.{0,70}$') {
        return 'the subject does not match the type(scope): subject format'
    }

    return $null
}
