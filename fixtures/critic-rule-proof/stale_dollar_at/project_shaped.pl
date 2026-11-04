use strict;
use warnings;

package App::Runner;

sub run {
    my ($body) = @_;
    my $ok = eval { $body->(); 1 };
    if (!$ok) {
        warn 'run failed';
        return 0;
    }
    return 1;
}

1;
