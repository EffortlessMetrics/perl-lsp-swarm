use strict;
use warnings;

my $count = 1;
my @items = qw(alpha beta gamma);
$count += scalar @items;
print "$count\n";
