package Greeting;
# Fixture module for the LSP4IJ real-host journey: one exported function with
# a documented return so hover, references, and symbol taps have stable
# subjects.
use strict;
use warnings;

sub greet {
    my ($name) = @_;
    return "hello, $name";
}

1;
