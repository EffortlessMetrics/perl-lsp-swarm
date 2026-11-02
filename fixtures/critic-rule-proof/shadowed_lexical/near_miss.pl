use strict;
use warnings;
my $outer = 1;
{ my $inner = 2; print $inner; }
print $outer;
