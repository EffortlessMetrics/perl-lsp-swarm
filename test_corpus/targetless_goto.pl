#!/usr/bin/env perl
# Test: Targetless goto (bare `goto;`) in realistic guard/retry shapes
# NodeKinds: TargetlessGoto (bare, short-circuit, and statement-modifier
#           omissions), Goto (labeled and sub forms, for contrast)
#
# Targetless goto is legal Perl at parse time: omitting the target makes the
# transfer restart the currently executing subroutine at run time (#15879).
# This fixture exercises parse-level acceptance only; none of the transfers
# are executed, and no label or operand is fabricated for the bare forms.

use strict;
use warnings;

my $attempts = 0;

sub render_once {
    my ($depth) = @_;
    $attempts++;
    return "depth $depth" if $depth > 0;

    # Statement-modifier targetless form: retry the current sub.
    goto if $attempts < 3;
    return "render after $attempts";
}

sub drain_queue {
    my @queue = @_;
    while (@queue) {
        my $item = shift @queue;
        return $item if $item % 2;

        # Short-circuit targetless form inside a loop guard.
        @queue == 0 and goto;
    }
    return ();
}

sub dispatch_event {
    my ($event) = @_;
    LOOP: {
        if ($event eq 'restart') {
            # Bare targetless form with a label in scope.
            goto;
        }
        if ($event eq 'reroute') {
            # Targeted label form, for contrast.
            goto LOOP;
        }
        if ($event eq 'tailcall') {
            # Targeted sub form, for contrast.
            goto &dispatch_event;
        }
    }
    return "handled $event";
}

print render_once(1), "\n";
print scalar(drain_queue(2, 3)), "\n";
print dispatch_event('reroute'), "\n";
