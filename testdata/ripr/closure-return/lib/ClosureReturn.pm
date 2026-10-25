package ClosureReturn;
use strict;
use warnings;

sub direct {
    my ($changed) = @_;
    my $n = $changed ? 5 : 40;
    return sub { $n++ };
}

sub bound {
    my $n = 5;
    my $next = sub { $n++ };
    return $next;
}

sub conditional {
    my ($pick) = @_;
    return $pick ? sub { 5 } : sub { 40 };
}

sub evaluated {
    return eval 'sub { 5 }';
}

1;
