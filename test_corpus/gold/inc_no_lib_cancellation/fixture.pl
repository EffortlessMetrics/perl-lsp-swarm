use strict;
use warnings;
use lib 'lib';
no lib 'lib';
# denom-target:gone-module-use-line
use GoneModule;

print "unreachable\n";
