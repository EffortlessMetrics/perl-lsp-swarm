#!/usr/bin/perl
use strict;
use warnings;
use lib 'lib';
use Utils;
use Database;

my $data = Utils::load_data();
my $summary = Utils::process_data($data);
Database::save($summary);

print "Done\n";
