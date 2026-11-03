use strict;
use warnings;
my $str = "hello";
$str =~ /(\w+)/;
{
    my $inner = $1;
}
