# Source subjects for #17532; no test framework or I/O.
package RuntimeCohort;

sub arithmetic {
    my ($a, $b, $c) = @_;
    return ($a + $b) * $c - $b;
}

sub comparisons {
    my ($a, $b) = @_;
    return ($a == $b ? 1 : 0, $a eq $b ? 1 : 0);
}

sub truth_and_definedness {
    return (defined $_[0] ? 1 : 0, $_[0] ? 1 : 0);
}

sub default_value {
    return $_[0] // 'fallback';
}

sub increment_alias {
    ++$_[0];
    return $_[0];
}

sub numeric_then_increment {
    my $number = 0 + $_[0];
    ++$_[0];
    return $number;
}

1;
