# Self-resolution fold stage of `just _public-api-filter` (#16324).
#
# Resolves method-signature `Self` to the owning type path
# (`pub fn krate::Type::clone(&self) -> Self` folds to
# `... -> krate::Type`) so August full-path and September `Self`
# nightly rustdoc-JSON renderings converge to the stored full-path
# canonical form. Both sides of the comparison go through this fold,
# so a real rename still diffs: the owner derives from the line itself.
#
# Owner scan: the method paren is the first "(" at generic depth 0,
# and the owner ends at the last "::" before it. A greedy `.*::`
# regex overshoots into argument lists (`Vec<(` in args reads as a
# method paren and mints a garbage owner), so scan with `<>` depth
# instead. `->` is skipped as a two-character token: function-trait
# bounds like `Fn() -> Output` carry an arrow whose `>` must not
# close the generic depth. A "(" nested inside generics (fn-pointer
# type arguments) bails the line out unfolded rather than guessing
# an owner. Attribute-fronted items (`#[must_use] pub fn ...`) fold
# on the signature following the attribute run.
#
# A `Self`-looking substring inside a longer identifier (e.g.
# `Selfish`) keeps its spelling via the boundary check. Failure
# direction is a visible phantom diff, never a silently hidden
# rename.
/^(#[^]]*\][[:space:]]*)*pub fn / {
    line = $0
    match(line, /pub fn /)
    start = RSTART + 7
    n = length(line)
    depth = 0
    lastcolon = 0
    paren = 0
    for (i = start; i <= n; i++) {
        c = substr(line, i, 1)
        if (c == "-" && substr(line, i, 2) == "->") { i++ }
        else if (c == "<") depth++
        else if (c == ">") { if (depth > 0) depth-- }
        else if (c == "(" && depth == 0) { paren = i; break }
        else if (c == ":" && substr(line, i, 2) == "::" && depth == 0) { lastcolon = i; i++ }
    }
    if (paren > 0 && lastcolon > start) {
        head = substr(line, start, lastcolon - start)
        m = split(line, part, /Self/)
        line = part[1]
        for (j = 2; j <= m; j++) {
            before = substr(line, length(line), 1)
            after = substr(part[j], 1, 1)
            if ((before == "" || before ~ /[^A-Za-z0-9_:]/) && (after == "" || after ~ /[^A-Za-z0-9_]/)) {
                line = line head part[j]
            } else {
                line = line "Self" part[j]
            }
        }
    }
    print line
    next
}
{ print }
