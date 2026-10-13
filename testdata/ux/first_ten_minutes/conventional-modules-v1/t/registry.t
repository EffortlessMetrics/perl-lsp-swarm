use strict;
use warnings;

use lib 'lib';
use Test::More;

use_ok 'App::Registry';

my $registry = App::Registry->new;
isa_ok $registry, 'App::Registry';

$registry->register( greeting => 'hello' );
is $registry->lookup('greeting'), 'hello', 'registered value is returned';
is_deeply [ $registry->names ], ['greeting'], 'registry names are sorted';

done_testing;
