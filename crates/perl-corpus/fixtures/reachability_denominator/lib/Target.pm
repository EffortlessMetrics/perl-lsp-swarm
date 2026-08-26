# Reachability denominator subject W1 target module.
package Target;
use strict;
use warnings;

sub run { return "run:@_"; }
sub measure { return "measure:@_"; }
sub build { return bless {}, shift; }

1;
