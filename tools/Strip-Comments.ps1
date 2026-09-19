#requires -Version 7
# PostToolUse hook for Claude Code (runs after Edit/Write tool calls).
# Reads the tool-call JSON payload from stdin; if the touched file is a
# .rs file, rewrites it in place to enforce the project comment policy:
#
#   - a standalone // comment is kept only in the exact "// WHY: ..." form,
#     other standalone // comment lines are removed;
#   - a /// doc-comment block is truncated to its first 2 lines;
#   - a trailing // comment after code is stripped when the scanner is
#     confident it is outside string literals.
#
# The scanner is line-based (not a full Rust tokenizer) but tracks double
# quotes, char literals and raw strings across lines; when genuinely unsure
# about a line, the line is left untouched rather than mangled. This script
# never fails the tool call: it always exits 0, even when it does nothing.

$ErrorActionPreference = 'Stop'
try {
    $rawJson = [Console]::In.ReadToEnd()
    if (-not [string]::IsNullOrWhiteSpace($rawJson)) {
        $payload = $rawJson | ConvertFrom-Json -Depth 32

        # Claude Code PostToolUse payload puts the path in
        # tool_input.file_path; a top-level file_path is also accepted.
        $path = $null
        if ($payload.PSObject.Properties['tool_input'] -and $payload.tool_input) {
            $toolInput = $payload.tool_input
            if ($toolInput.PSObject.Properties['file_path']) {
                $path = [string]$toolInput.file_path
            }
        }
        if (-not $path -and $payload.PSObject.Properties['file_path']) {
            $path = [string]$payload.file_path
        }

        if ($path -and $path.EndsWith('.rs') -and (Test-Path -LiteralPath $path -PathType Leaf)) {
            $bytes = [System.IO.File]::ReadAllBytes($path)
            # Strict UTF-8 decode: a non-UTF-8 file aborts via the catch below.
            $text = [System.Text.UTF8Encoding]::new($false, $true).GetString($bytes)
            $hadFinalNewline = $text.EndsWith("`n")
            $lines = $text -split "`n"

            $out = [System.Collections.Generic.List[string]]::new()
            $state = 'none'
            $rawHashes = 0
            $docRun = 0

            foreach ($line in $lines) {
                $hadCr = $line.EndsWith("`r")
                $core = if ($hadCr) { $line.Substring(0, $line.Length - 1) } else { $line }
                $ending = if ($hadCr) { "`r" } else { '' }

                # Scan for the first // that is outside string literals.
                $cidx = -1
                $i = 0
                $n = $core.Length
                while ($i -lt $n) {
                    $ch = $core[$i]
                    if ($state -eq 'dq') {
                        if ($ch -eq '\') { $i += 2; continue }
                        elseif ($ch -eq '"') { $state = 'none' }
                    } elseif ($state -eq 'sq') {
                        if ($ch -eq '\') { $i += 2; continue }
                        elseif ($ch -eq "'") { $state = 'none' }
                    } elseif ($state -eq 'raw') {
                        if ($ch -eq '"') {
                            $closes = $true
                            for ($k = 1; $k -le $rawHashes; $k++) {
                                if (($i + $k) -ge $n -or $core[$i + $k] -ne '#') {
                                    $closes = $false
                                    break
                                }
                            }
                            if ($closes) {
                                $state = 'none'
                                $i += 1 + $rawHashes
                                continue
                            }
                        }
                    } else {
                        if ($ch -eq 'r') {
                            $j = $i + 1
                            $h = 0
                            while (($j -lt $n) -and ($core[$j] -eq '#')) { $h++; $j++ }
                            if (($j -lt $n) -and ($core[$j] -eq '"')) {
                                $state = 'raw'
                                $rawHashes = $h
                                $i = $j + 1
                                continue
                            }
                        }
                        if ($ch -eq '"') { $state = 'dq' }
                        elseif ($ch -eq "'") { $state = 'sq' }
                        elseif ($ch -eq '/' -and (($i + 1) -lt $n) -and ($core[$i + 1] -eq '/')) {
                            $cidx = $i
                            break
                        }
                    }
                    $i++
                }

                if ($cidx -lt 0) {
                    # No comment on this line: code, blank or string content.
                    $out.Add($line)
                    $docRun = 0
                    continue
                }

                $before = $core.Substring(0, $cidx)
                $rest = $core.Substring($cidx)
                $isDoc = $rest.StartsWith('///') -and
                    (($rest.Length -eq 3) -or ($rest[3] -ne '/'))
                $isStandalone = $before.Trim().Length -eq 0

                if ($isDoc) {
                    if ($isStandalone) {
                        $docRun++
                        if ($docRun -le 2) { $out.Add($line) }
                        # 3rd and later lines of the block are dropped.
                    } else {
                        # Not valid Rust anyway; leave untouched.
                        $out.Add($line)
                        $docRun = 0
                    }
                    continue
                }

                $docRun = 0
                if ($isStandalone) {
                    $trimmed = $rest.TrimEnd()
                    if ($trimmed -match '^// WHY: [^ \t]') {
                        $out.Add($line)
                    }
                    # Any other standalone // comment line is removed.
                } else {
                    # Trailing comment after code: strip it, keep the code
                    # and the original line ending.
                    $out.Add($before.TrimEnd() + $ending)
                }
            }

            $newText = $out -join "`n"
            if ($hadFinalNewline) { $newText += "`n" }
            if ($newText -ne $text) {
                [System.IO.File]::WriteAllText(
                    $path, $newText, [System.Text.UTF8Encoding]::new($false))
            }
        }
    }
} catch {
    # Never fail the tool call; the file is left as-is on any error.
}
exit 0
