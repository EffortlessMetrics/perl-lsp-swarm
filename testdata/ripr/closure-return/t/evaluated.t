use strict;
use warnings;
use Test::More;
use ClosureReturn;

my $c = ClosureReturn::evaluated();
is($c->(), 5, 'runtime eval works but static return must remain dynamic');
done_testing;
