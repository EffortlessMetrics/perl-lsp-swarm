package Example::Product;
use strict;
use warnings;
sub new { return bless {}, shift }
sub finish { return 1 }
1;
