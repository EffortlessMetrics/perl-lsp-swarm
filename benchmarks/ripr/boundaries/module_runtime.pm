package ModuleRuntimeLoader;
use strict;
use warnings;

sub load {
    use_module('My::Plugin');
    require_module('My::Other');
    return 1;
}

1;
