package Database;
use strict;
use warnings;

# Print the result so this example needs no database or extra Perl modules.
sub save {
    my ($data) = @_;
    print "Saving summary: min=$data->{min}, max=$data->{max}, sum=$data->{sum}\n";
    return 1;
}

1;
