use strict;
use warnings;
my $path = 'file.txt';
## no critic native.io.two_arg_open -- trusted legacy filename
open(my $fh, $path);
