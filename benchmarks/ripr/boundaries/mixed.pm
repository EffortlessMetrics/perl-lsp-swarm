package Mixed;
use strict;
use warnings;

our $AUTOLOAD;
our @ISA;

sub AUTOLOAD {
    my $method = $AUTOLOAD;
    return $method;
}

sub run {
    my ($code) = @_;
    my $result = eval"$code";
    return $result;
}

sub call {
    my ($obj, $method) = @_;
    return $obj->$method();
}

sub extend {
    my ($parent) = @_;
    push @ISA, $parent;
    return scalar @ISA;
}

1;
