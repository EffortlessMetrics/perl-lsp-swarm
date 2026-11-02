use strict;
use warnings;
## no critic native.variables.parameter_shadows_global -- fixture
my $name = 'outer';
sub helper($name) { return $name; }
print $name;
