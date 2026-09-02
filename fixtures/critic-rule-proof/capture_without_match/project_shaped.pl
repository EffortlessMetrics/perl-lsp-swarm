use strict;
use warnings;

package Clean::RegexCaptureGuarded;

sub first_word {
    my ($text) = @_;
    if ($text =~ /(\w+)/) {
        return $1;
    }
    return;
}

1;
