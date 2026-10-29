use strict;
use warnings;

package App::Reader;

sub load {
    my ($path) = @_;
    open(my $fh, '<', $path) or die $!;
    return $fh;
}

1;
