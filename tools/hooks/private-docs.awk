# Blocks a /// block unless it sits on a pub item or a trait declaration
# member (documented once, on the trait). Clap derive items are skipped
# too: their /// becomes --help text. Trait impl methods are blocked.
function braces(s,   t, o, c) { t = s; o = gsub(/\{/, "", t); t = s; c = gsub(/\}/, "", t); return o - c }
{ sub(/\r$/, "") }
{ line = $0; before = depth; depth += braces(line) }
{
    if (sp > 0 && depth <= stack[sp]) sp--
    if (line ~ /^[ \t]*(pub[^ ]* )?(unsafe )?trait[ \t]/) {
        if (depth > before) stack[++sp] = before
        else pendtrait = 1
    } else if (pendtrait && depth > before) { stack[++sp] = before; pendtrait = 0 }
}
line ~ /^[ \t]*\/\/\/([^\/]|$)/ { indoc = 1; start = NR; next }
indoc == 0 { next }
inattr { if (line ~ /\][ \t]*$/) inattr = 0; next }
line ~ /^[ \t]*#\[/ {
    if (line ~ /derive\(.*(Parser|Subcommand|ValueEnum|Args)/) clap = 1
    if (line !~ /\][ \t]*$/) inattr = 1
    next
}
line ~ /^[ \t]*$/ { next }
{
    item = line; sub(/^[ \t]+/, "", item)
    intrait = (sp > 0 && before > stack[sp])
    if (!clap && !intrait && item ~ /^((async|const|unsafe|extern)[ \t]+)*(fn|struct|enum|const|static|type|trait|mod|union)[ \t]/) {
        printf "%s:%d: /// allowed only on pub items and trait declarations\n", f, start > "/dev/stderr"
        bad = 1
    }
    indoc = 0; clap = 0
}
END { if (bad) exit 1 }