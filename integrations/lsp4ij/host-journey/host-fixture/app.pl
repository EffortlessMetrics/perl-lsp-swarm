#!/usr/bin/env perl
# Fixture subject for the LSP4IJ real-host journey: one plain .pl script that
# exercises package symbols, a method call, and a here-doc-free deterministic
# print so first-diagnostics settlement is reproducible.
use strict;
use warnings;

use lib 'lib';
use Greeting;

my $target = shift @ARGV || 'host';
print Greeting::greet($target), "\n";
