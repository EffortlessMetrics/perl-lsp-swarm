use strict;
use warnings;

my @events;

push @events, 'if' if 1;
push @events, 'unless' unless 0;

my $while = 0;
push @events, "while:$while" while $while++ < 2;

my $until = 0;
push @events, "until:$until" until $until++ >= 2;

my @aliased = qw(alpha beta);
$_ = uc $_ for @aliased;
push @events, 'for:' . join(',', @aliased);

my @foreach = (1, 2, 3);
push @events, "foreach:$_" foreach @foreach;

print join('|', @events), "\n";
