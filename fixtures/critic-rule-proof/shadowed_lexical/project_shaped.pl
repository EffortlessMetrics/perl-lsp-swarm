use strict;
use warnings;

package App::Block;

sub with_temp {
    my ($code) = @_;
    { local $_ = 1; return $code->(); }
}

1;
