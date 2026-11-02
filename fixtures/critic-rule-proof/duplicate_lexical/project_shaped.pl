use strict;
use warnings;

package App::State;

sub counter {
    state $count = 0;
    return ++$count;
}

1;
