package Accuracy::TieInterface;

sub configure {
    tie my %cache, 'Tie::StdHash', initial => 0;
    my $object = tied %cache;
    $cache{ready} = 1;
    untie %cache;
    return $object;
}

1;
