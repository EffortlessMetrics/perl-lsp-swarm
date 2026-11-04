use strict;
use warnings;

package App::Flow;

sub run {
    my ($flag) = @_;
    return 0 if !$flag;
    eval { risky_call(); };
    return 1;
}

1;
