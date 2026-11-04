package SymTable;
use strict;
use warnings;

sub install {
    my ($name, $code) = @_;
    no warnings 'redefine';
    *{"SymTable::$name"} = $code;
    return $name;
}

1;
