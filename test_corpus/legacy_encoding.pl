# Latin-1 encoded Perl file: café (U+00E9 = 0xE9 in Latin-1)
# Used to verify LSP handles non-UTF8 legacy Perl codebases.
my $greeting = "Bonjour";
my $x = 42;
print "$greeting: $x
";
1;
