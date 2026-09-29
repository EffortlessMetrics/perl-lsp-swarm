# Reachability denominator subject G2: test-side caller for production/test
# closure partitioning over GraphShapes.
# Declared by fixtures/analysis_reachability_denominator/manifest.json (#10998).
use strict;
use warnings;
use lib 'lib';
use GraphShapes;

my $probe = GraphShapes::isolated_never_called();
my $chain = GraphShapes::chain_a();

print "$probe $chain\n";
