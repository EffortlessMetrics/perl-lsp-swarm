#!/usr/bin/perl
use strict;
use warnings;

sub calculate_total {
    my ($left, $right) = @_;
    return $left + $right;
}

my $first = calculate_total(2, 3);
my $second = calculate_total(5, 8);
print "$first $second\n";
