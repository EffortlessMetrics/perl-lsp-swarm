package Autoload;
use strict;
use warnings;

our $AUTOLOAD;

sub AUTOLOAD {
    my $method = $AUTOLOAD;
    return "handled $method";
}

sub DESTROY { }

1;
