use strict;
# gap-matrix: completion_branch_join_receiver_union
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
my $use_http = shift @ARGV;  # runtime-unknown: both join arms stay feasible
my $client;
if ($use_http) {
    $client = HttpClient->new;
} else {
    $client = MockClient->new;
}
$client->request;
