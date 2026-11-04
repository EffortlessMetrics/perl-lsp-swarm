use strict;
use warnings;
sub check {
    my ($flag) = @_;
    return 1 if $flag;
    my $live = 2;
    return $live;
}
