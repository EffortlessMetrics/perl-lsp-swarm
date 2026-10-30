use strict;
use warnings;
## no critic native.io.unchecked_open_close -- handled by caller
my $path = 'file.txt';
open(my $fh, '<', $path);
