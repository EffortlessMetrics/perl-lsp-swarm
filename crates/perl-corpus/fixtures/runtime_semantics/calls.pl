# Source subjects for #17532; no test framework or I/O.
package RuntimeCohort;

sub update_pair {
    $_[0] += 2;
    $_[1] *= 3;
    return ($_[0], $_[1]);
}

sub update_copy {
    my $copy = $_[0];
    $copy += 5;
    return $copy;
}

sub call_term {
    return ($_[0] + $_[1]) * $_[2] - $_[1];
}

sub nested_call {
    my $x = $_[0];
    return call_term($x, 2, 3) + call_term($x, 1, 2);
}

sub factorial {
    my $n = $_[0];
    if ($n <= 1) { return 1; }
    return $n * factorial($n - 1);
}

sub return_values {
    return (11, 22, 33);
}

1;
