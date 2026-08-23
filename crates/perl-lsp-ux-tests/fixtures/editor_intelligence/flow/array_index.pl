use strict;
# gap-matrix: completion_array_index_receiver
# gap-matrix: array_index_not_promoted
use warnings;

package HttpClient;
sub new { return bless {}, shift }
sub request { return 1 }

package main;
my @clients = (HttpClient->new);
$clients[0]->request;
