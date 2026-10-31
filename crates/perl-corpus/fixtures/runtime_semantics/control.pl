# Source subjects for #17532; no test framework or I/O.
package RuntimeCohort;

sub touch {
    $_[0] += 1;
    return $_[1];
}

sub short_circuit {
    my ($condition, $value) = @_;
    my $touches = 0;
    my $result = $condition && touch($touches, $value);
    return ($result, $touches);
}

sub select_arm {
    my $touches = 0;
    my $result = $_[0] ? touch($touches, 'yes') : touch($touches, 'no');
    return ($result, $touches);
}

sub sum_loop {
    my $limit = $_[0];
    my $sum = 0;
    for (my $i = 0; $i < $limit; $i++) {
        $sum += $i;
    }
    return $sum;
}

sub next_last_continue {
    my $i = 0;
    my $trace = '';
    while ($i < 5) {
        $i++;
        next if $i == 2;
        last if $i == 4;
        $trace .= $i;
    } continue {
        $trace .= 'c';
    }
    return ($trace, $i);
}

sub redo_continue {
    my $iterations = 0;
    my $retried = 0;
    my $trace = '';
    while ($iterations < 2) {
        $trace .= 'b';
        if (!$retried) {
            $retried = 1;
            redo;
        }
        $iterations++;
    } continue {
        $trace .= 'c';
    }
    return ($trace, $iterations);
}

1;
