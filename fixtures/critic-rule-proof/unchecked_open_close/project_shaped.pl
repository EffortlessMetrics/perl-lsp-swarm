use strict;
use warnings;

package App::Store;

sub load {
    my ($path) = @_;
    open(my $fh, '<', $path) or die $!;
    close($fh) or die $!;
    return 1;
}

1;
