use strict;
use warnings;
use Test::More;
use ClosureReturn;

my $c = ClosureReturn::bound();
is($c->(), 5, 'arrow observes lexical return');
is(&$c(), 6, 'ampersand observes lexical return');
is(&{$c}(), 7, 'braced ampersand observes lexical return');
done_testing;
