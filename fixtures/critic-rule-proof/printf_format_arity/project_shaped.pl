use strict;
use warnings;

package App::Formatter;

sub greet {
    my ($name) = @_;
    return sprintf "%s", $name;
}

1;
