package EvalString;
use strict;
use warnings;

sub run {
    my ($code) = @_;
    my $result = eval"$code";
    return $result;
}

1;
