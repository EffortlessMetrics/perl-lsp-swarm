use strict;
use warnings;

package HttpClient;
sub new { return bless {}, shift }
sub request { return 1 }

package main;
my @clients = (HttpClient->new);
$clients[0]->request;
