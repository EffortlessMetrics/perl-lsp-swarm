use strict;
use warnings;

use lib 'lib';
use Test::More;
use Inventory;

subtest 'missing skus count as zero' => sub {
    plan tests => 1;
    my $inventory = Inventory->new;
    is $inventory->count('nope'), 0, 'unknown sku is zero';
};

subtest 'repeated adds accumulate' => sub {
    plan tests => 1;
    my $inventory = Inventory->new;
    $inventory->add( 'sku-2', $_ ) for 1 .. 3;
    is $inventory->count('sku-2'), 6, 'adds accumulate';
};

subtest 'skus are reported sorted' => sub {
    plan tests => 1;
    my $inventory = Inventory->new;
    $inventory->add( 'b', 1 );
    $inventory->add( 'a', 1 );
    is_deeply [ $inventory->skus ], [ 'a', 'b' ], 'sorted sku list';
};

done_testing;
