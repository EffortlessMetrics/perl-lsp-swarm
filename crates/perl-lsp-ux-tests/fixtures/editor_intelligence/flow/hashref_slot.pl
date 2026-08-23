use strict;
# gap-matrix: completion_hashref_slot_receiver
# gap-matrix: hashref_slot_not_promoted
use warnings;

package HttpClient;
sub new { return bless {}, shift }
sub request { return 1 }

package main;
my $services = { db => HttpClient->new };
$services->{db}->request;
