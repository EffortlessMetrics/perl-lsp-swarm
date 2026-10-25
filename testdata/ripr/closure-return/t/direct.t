use strict;
use warnings;
use Test::More;
use ClosureReturn;

my $c = ClosureReturn::direct(1);
is($c->(), 5, 'arrow observes changed capture');
is(&$c(), 6, 'ampersand observes same capture');
is(&{$c}(), 7, 'braced ampersand observes same capture');
done_testing;
