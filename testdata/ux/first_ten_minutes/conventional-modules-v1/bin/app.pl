#!/usr/bin/perl
use strict;
use warnings;
use lib 'lib';

use App::Format;
use App::Registry;

my $registry = App::Registry->new;
$registry->register( greeting => 'hello' );
$registry->register( target   => 'world' );

my @rows = map { [ $_, $registry->lookup($_) ] } $registry->names;
print App::Format->table( \@rows );
print "\n";
