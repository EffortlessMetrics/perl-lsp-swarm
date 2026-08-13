use strict;
use warnings;

package HttpClient;
sub new { return bless {}, shift }
sub request { return 1 }
sub close { return 1 }

package MockClient;
sub new { return bless {}, shift }
sub request { return 1 }
sub reset { return 1 }

package main;
my $use_http = 1;
my $client;
if ($use_http) {
    $client = HttpClient->new;
} else {
    $client = MockClient->new;
}
$client->request;
