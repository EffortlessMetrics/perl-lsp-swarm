package StrDispatch;
use strict;
use warnings;

sub call {
    my ($obj, $method, @args) = @_;
    return $obj->$method(@args);
}

1;
