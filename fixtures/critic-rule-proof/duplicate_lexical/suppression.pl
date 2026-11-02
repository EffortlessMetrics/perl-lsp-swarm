use strict;
use warnings;
## no perl-lsp-critic native.variables.duplicate_lexical -- fixture
my $dup = 1;
my $dup = 2;
print $dup;
