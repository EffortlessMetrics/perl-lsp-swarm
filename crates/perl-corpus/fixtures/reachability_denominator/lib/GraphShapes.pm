# Reachability denominator subject G1: graph shape subjects.
# Isolated node, chain, self-loop, dead SCC and live SCC in one module.
# Declared by fixtures/analysis_reachability_denominator/manifest.json (#10998).
# The module lives at lib/GraphShapes.pm so normal Perl module discovery
# resolves `use GraphShapes` from sibling scripts (PR #12706 review fix).
package GraphShapes;
use strict;
use warnings;

sub isolated_never_called { return 'island'; }

sub chain_a { return chain_b(); }
sub chain_b { return chain_c(); }
sub chain_c { return 'chain-end'; }

sub self_loop { return self_loop(); }

sub dead_scc_left  { return dead_scc_right(); }
sub dead_scc_right { return dead_scc_left(); }

# g05 truth: entry_calls_live_scc plus live_scc_left/right form one strongly
# connected component because live_scc_left calls back into the entry caller.
sub live_scc_left {
    my $next = live_scc_right();
    return defined($next) ? $next : entry_calls_live_scc();
}
sub live_scc_right { return live_scc_left(); }

sub entry_calls_live_scc { return live_scc_left(); }

# denom-target:missing-family-request

1;
