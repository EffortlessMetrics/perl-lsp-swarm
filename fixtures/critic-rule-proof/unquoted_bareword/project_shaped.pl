use strict;
use warnings;

package App::Config;

my %defaults = (mode => 'ro');

sub mode {
    return $defaults{mode};
}

1;
