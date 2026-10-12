# Source subjects for #17532; no test framework or I/O.
package RuntimeCohort;

# Later-family obligations; not admission of eval/local into v1.
our $Dynamic;

sub effect_before_failure {
    local $Dynamic = 'outer';
    my $ok = eval {
        local $Dynamic = 'inner';
        $_[0] += 3;
        die "cohort-stop\n";
        $_[0] += 100;
    };
    my $error = $@;
    return ($ok, $_[0], $Dynamic, $error);
}

sub nested_localization {
    local $Dynamic = 'outer';
    my $trace = '';
    eval {
        local $Dynamic = 'middle';
        eval {
            local $Dynamic = 'inner';
            die "inner-stop\n";
        };
        $trace .= $Dynamic;
        die "outer-stop\n";
    };
    my $error = $@;
    return ($trace, $Dynamic, $error);
}

sub return_from_local {
    local $Dynamic = 'temporary';
    return $Dynamic;
}

1;
