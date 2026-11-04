package EvalSpaced;
use strict;
use warnings;

sub run {
    my ($code) = @_;
    my $result = eval $code;
    return $result;
}

sub quoted {
    my ($code) = @_;
    return eval "$code";
}

1;
