#!/usr/bin/env perl
# Fixture test subject: kept green by construction so a hosted run only has
# to prove the LSP4IJ->perllsp round trip, not Perl itself.
use strict;
use warnings;

use Test::More tests => 1;

use lib 'lib';
use Greeting;

ok(Greeting::greet('t') eq "hello, t", 'greet returns the expected string');
done_testing();
