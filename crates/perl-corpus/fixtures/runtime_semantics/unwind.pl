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

# Repeated alias arguments share one cell; both writes land exactly once
# before the caught failure. Sequential statements: well-defined order.
sub shared_alias_before_failure {
    my $ok = eval {
        $_[0] += 3;
        $_[1] += 5;
        die "shared-stop\n";
        $_[0] += 100;
    };
    my $error = $@;
    return ($ok, $_[0], $error);
}

sub dies_after_local_mark {
    local $Dynamic = 'callee-mark';
    die "callee-stop\n";
}

# Callee's localization unwinds through the caller's catching eval while
# the caller's own localization stays intact.
sub caller_local_catches_callee {
    local $Dynamic = 'caller-mark';
    my $ok = eval { dies_after_local_mark(); };
    my $error = $@;
    return ($ok, $Dynamic, $error);
}

# Successful eval returns the block value and clears the error channel.
sub clean_eval_result {
    my $ok = eval { $_[0] += 2; 'done-mark'; };
    my $error = $@;
    return ($ok, $_[0], $error);
}

1;
