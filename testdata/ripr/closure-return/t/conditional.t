use strict;
use warnings;
use Test::More;
use ClosureReturn;

my $left = ClosureReturn::conditional(1);
my $right = ClosureReturn::conditional(0);
is($left->(), 5, 'first selected body');
is($right->(), 40, 'second selected body');
done_testing;
