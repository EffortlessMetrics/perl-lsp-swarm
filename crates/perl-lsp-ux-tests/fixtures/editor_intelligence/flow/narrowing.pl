use strict;
use warnings;

package HttpClient;
sub new { return bless {}, shift }
sub request { return 1 }
sub close { return 1 }

package main;
my $value = {};
if (ref($value) eq 'HASH') {
    my $host = $value->{host};
}

my $client = HttpClient->new;
if ($client->isa('HttpClient')) {
    $client->close;
}
if (defined $client) {
    $client->request;
}
