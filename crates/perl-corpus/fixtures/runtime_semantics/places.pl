# Source subjects for #17532; no test framework or I/O.
package RuntimeCohort;

sub index_once {
    $_[0] += 1;
    return 0;
}

sub modify_place {
    my @values = (3, 4);
    my $indices = 0;
    $values[index_once($indices)] += 7;
    return (@values, $indices);
}

sub foreach_alias {
    my @values = (1, 2, 3);
    for my $value (@values) { $value *= 2; }
    return @values;
}

1;
