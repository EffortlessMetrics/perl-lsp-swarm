package DynRequire;
use strict;
use warnings;

sub load {
    my ($module) = @_;
    require $module;
    return $module;
}

1;
