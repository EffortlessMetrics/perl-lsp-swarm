use strict;
use warnings;
use Test::More;
use ClosureReturn;

my $changed = ClosureReturn::direct(1);
my $other = ClosureReturn::direct(0);
$changed->();
is($other->(), 40, 'other factory instance does not observe changed initializer');
done_testing;
