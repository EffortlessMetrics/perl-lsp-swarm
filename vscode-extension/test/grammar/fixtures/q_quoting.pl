my $escaped = q(a\)b);
my @w = qw(a (b) c);
my $nested = q{ {x} };
my $bracket = q[ [x] ]; ok;
my $angle = q< <tag> >; end;
my $slash = q/a\/b/; tail;
my $early = q(a) . (b);
