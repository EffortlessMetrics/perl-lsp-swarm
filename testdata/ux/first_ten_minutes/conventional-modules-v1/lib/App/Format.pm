package App::Format;
use strict;
use warnings;

sub table {
    my ($class, $rows) = @_;
    return join "\n", map { sprintf '%-12s | %s', $_->[0], $_->[1] } @{$rows};
}

1;
