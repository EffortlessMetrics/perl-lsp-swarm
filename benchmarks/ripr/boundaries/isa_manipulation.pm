package IsaManip;
use strict;
use warnings;

our @ISA;

sub extend {
    my ($parent) = @_;
    push @ISA, $parent;
    return scalar @ISA;
}

1;
