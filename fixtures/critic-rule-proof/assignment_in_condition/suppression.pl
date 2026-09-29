use strict;
use warnings;
my $x = 0;
## no critic native.common.assignment_in_condition -- intentional assign-and-test
if ($x = 5) { print $x; }
