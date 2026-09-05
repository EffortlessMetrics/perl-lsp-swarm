use strict;
use warnings;

package App::SafeEval;

sub try_load {
    my ($body) = @_;
    eval {
        $body->();
        1;
    } or do {
        return 0;
    };
    return 1;
}

1;
