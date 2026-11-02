use strict;
use warnings;

package App::Reduce;

sub fold {
    my ($acc, $item) = @_;
    return $acc + $item;
}

1;
