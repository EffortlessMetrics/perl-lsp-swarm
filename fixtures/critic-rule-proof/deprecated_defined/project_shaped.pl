use strict;
use warnings;

package App::Items;

sub has_items {
    my (@items) = @_;
    if (@items) { return 1; }
    return 0;
}

1;
