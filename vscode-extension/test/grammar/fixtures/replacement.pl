$str =~ s/pattern/replacement/gir; my $after = 1;
$str =~ s///r; my $empty = 1;
$str =~ s/foo//ee; my $flags = 1;
$str =~ s{pattern}{replacement}gir; my $paired = 1;
$str =~ s{a{2}}{b{c}}r; my $nested = 1;
$str =~ tr[a-z][A-Z]cdr; my $translated = 1;
$str =~ y/a-z/A-Z/r; my $alias = 1;
$str =~ s/a\/b/c\/d/ee; my $escaped = 1;
$str =~ s{a\}b}{c\}d}r; my $brace_escape = 1;
$str =~ tr[a\]b][c\]d]r; my $bracket_escape = 1;
$str =~ s/foo/$value @array &literal my print/ee; my $lexical = 1;
$str =~ s!foo!bar!r; my $custom = 1;
$str =~ s(foo)(bar)r; my $paren = 1;
$str =~ s<foo><bar>r; my $angle = 1;
$str =~ s{foo} my $missing_opener = 1;
$str =~ s/foo/bar
/r; my $recovered = 1;
$str =~ tr[a-z][A-Z
]r; my $paired_recovered = 1;
$str =~ s/foo/bar/g; $str =~ s/baz/qux/r; m/next/i;
$str =~ s{foo}/bar/r; my $mixed = 1;
$str =~ s{foo}[bar]r; my $mixed_pair = 1;
$str =~ s{foo}
{bar}r; my $between_halves = 1;
$str =~ tr[a-z]
[A-Z]r; my $between_translation_halves = 1;
