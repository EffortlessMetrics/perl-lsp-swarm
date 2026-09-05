use strict;
use warnings;

sub next_value { 1 }

my $x = 0;
if (($x = next_value())) {
    print $x;
}
