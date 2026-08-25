#!/usr/bin/perl
use strict;
use warnings;

# Corpus fixture for clean-parse coverage of NodeKind::ChainedComparison
# (`0 <= $x < 100`, Perl 5.32+ chained relational comparison). The only other
# corpus occurrence sits in a file with unrelated diagnostics, so this kind
# was previously observed exclusively through recovery parses.

package Corpus::Coverage::ChainedComparison;

sub range_check {
    my ($x) = @_;

    # ChainedComparison as an `if` condition.
    if (0 <= $x < 100) {
        return 'in range';
    }

    # ChainedComparison as an assignment right-hand side.
    my $ordered = $x <= $x + 1 <= $x + 2;

    return $ordered ? 'ordered' : 'unordered';
}

1;
