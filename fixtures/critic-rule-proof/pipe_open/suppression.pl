use strict;
use warnings;
## no critic native.io.pipe_open -- trusted command
open(my $fh, '-|', 'ls');
