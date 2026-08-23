use strict;
use warnings;
use lib 'lib';
use Widget;

my $widget = Widget->new("Ada");
print $widget->greet(), "\n";
