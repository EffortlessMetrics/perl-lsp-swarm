# Reachability denominator subject G1: graph shape subjects.
# Isolated node, chain, self-loop, dead SCC and live SCC in one module.
# Declared by fixtures/analysis_reachability_denominator/manifest.json (#10998).
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

sub live_scc_left  { return live_scc_right(); }
sub live_scc_right { return live_scc_left(); }

sub entry_calls_live_scc { return live_scc_left(); }

1;
