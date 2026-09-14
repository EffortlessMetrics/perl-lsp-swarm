use strict;
use warnings;

use lib 'lib';
use Test::More tests => 4;

use_ok 'Inventory';

my $inventory = Inventory->new;
isa_ok $inventory, 'Inventory';

is $inventory->add( 'sku-1', 2 ), 2, 'add returns the new count';
is $inventory->count('sku-1'), 2, 'count reflects the added stock';
