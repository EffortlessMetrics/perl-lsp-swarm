package My::Module;

use strict;
use warnings;
use Exporter 'import';

our @EXPORT_OK = qw(answer);

sub answer {
    return 42;
}

1;
