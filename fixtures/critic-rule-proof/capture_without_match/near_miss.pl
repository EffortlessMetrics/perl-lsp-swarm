use strict;
use warnings;
if ('hello' =~ /(ell)/) {
    my $matched = $1;
    print $matched;
}
