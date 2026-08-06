#!/usr/bin/perl
use strict;
use warnings;
use lib 'lib';
use Utils;
use Database;

my $data = load_data();
my $processed = Utils::process_data($data);
Database::save($processed);

print "Done\n";

# Load sample data for the demo. In a real application this would read from
# a file, database, or API.
sub load_data {
    return [ 10, 20, 30, 40, 50 ];
}
