# Reachability denominator subject A1: exact local flow transfers and controls.
# Declared by fixtures/analysis_reachability_denominator/manifest.json (#10998).
use strict;
use warnings;

sub after_exit_never_runs {
    exit 0;
    print "unreachable after exit\n";
}

sub after_die_never_runs {
    die "terminal\n";
    print "unreachable after die\n";
}

sub after_return_never_runs {
    return 42;
    print "unreachable after return\n";
}

sub same_spelling_exit_is_user_sub {
    my $result = exit($result_unknown);
    print "reachable when exit is a user sub\n";
    return $result;
}

sub short_circuit_right_operand_skipped {
    my $left = shift;
    my $seen = 0;
    my $value = $left || ($seen = 1);
    return $seen ? "$value seen" : "$value skipped";
}

sub list_assignment_order {
    my @pairs = (first(), second());
    return join ',', map { $_ // 'undef' } @pairs;
}

sub nested_anonymous_callable {
    my $inner = sub { return 'inner'; };
    my $outer = sub { return $inner->(); };
    return $outer->();
}

sub block_taking_builtin {
    my @filtered = grep { defined } @_;
    my @mapped  = map { $_ * 2 } @filtered;
    return scalar @mapped;
}

LABEL: foreach my $item (1 .. 3) {
    next LABEL if $item == 2;
    last LABEL if $item == 3;
}

print "entry reachable\n";
