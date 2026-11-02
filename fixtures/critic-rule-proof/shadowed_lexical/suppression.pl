use strict;
use warnings;
## no critic native.variables.shadowed_lexical -- fixture
my $value = 1;
{ my $value = 2; print $value; }
print $value;
