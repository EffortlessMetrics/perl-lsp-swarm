# Reachability denominator subject A1: exact local flow transfers and controls.
# Declared by fixtures/analysis_reachability_denominator/manifest.json (#10998).
# Every denom-target label grounds a manifest subject fragment at real bytes.
use strict;
use warnings;

# A user-defined exit must be compiled before any call site that resolves it,
# or Perl binds the CORE builtin instead of this user sub (a02 control).
sub exit {
    my ($code) = @_;
    return "user-exit:$code";
}

my $result_unknown = 7;

sub after_exit_never_runs {
    # denom-target:after-exit
    exit 0;
    print "unreachable after exit\n";
}

sub after_die_never_runs {
    # denom-target:after-die
    die "terminal\n";
    print "unreachable after die\n";
}

sub after_return_never_runs {
    # denom-target:after-return
    return 42;
    print "unreachable after return\n";
}

sub same_spelling_exit_is_user_sub {
    # denom-target:same-spelling-exit-user-sub
    my $result = exit($result_unknown);
    print "reachable when exit is a user sub\n";
    return $result;
}

sub short_circuit_right_operand_skipped {
    # denom-target:short-circuit-right-skipped
    my $left = shift;
    my $seen = 0;
    my $value = $left || ($seen = 1);
    return $seen ? "$value seen" : "$value skipped";
}

sub list_assignment_order {
    # denom-target:list-assignment-order
    my @pairs = (first(), second());
    return join ',', map { $_ // 'undef' } @pairs;
}

sub nested_anonymous_callable {
    # denom-target:nested-anonymous
    my $inner = sub { return 'inner'; };
    my $outer = sub { return $inner->(); };
    return $outer->();
}

sub block_taking_builtin {
    # denom-target:grep-map-blocks
    my @filtered = grep { defined } @_;
    my @mapped  = map { $_ * 2 } @filtered;
    return scalar @mapped;
}

# a11 exact-process ceiling evidence: one real exec transfer site inside a
# named callable, called from reachable linear code.
sub exact_process_transfer {
    # denom-target:exec-site
    my @transfer_argv = @_;
    exec(@transfer_argv);
    print "never reached when exec succeeds\n";
    return 0;
}

exact_process_transfer() unless caller();

LABEL: foreach my $item (1 .. 3) {
    next LABEL if $item == 2;
    last LABEL if $item == 3;
}

print "entry reachable\n";
