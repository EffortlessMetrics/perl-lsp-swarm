use strict;
use warnings;
## no perl-lsp-critic native.variables.duplicate_parameter -- fixture
sub helper($arg, $arg) { return $arg; }
