package Utils;
use strict;
use warnings;
use List::Util qw(max min sum);

sub load_data {
    return [1, 2, 3, 4, 5];
}

sub process_data {
    my ($data) = @_;

    return {
        max => max(@$data),
        min => min(@$data),
        sum => sum(@$data),
    };
}

1;
