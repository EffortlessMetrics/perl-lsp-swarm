use strict;
use warnings;

my %pairs = (
    alpha => 1,
    beta  => 2,
);

my @entries = %pairs{qw(alpha beta)};
my $version = v1.2.3;
