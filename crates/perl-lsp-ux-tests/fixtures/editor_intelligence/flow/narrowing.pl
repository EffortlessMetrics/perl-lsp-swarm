use strict;
# gap-matrix: type_flow_ref_hash_narrowing type_flow_isa_narrowing type_flow_defined_narrowing
# gap-matrix: ref_narrowing_missing isa_narrowing_missing defined_narrowing_missing
use warnings;

package HttpClient;
sub new { return bless {}, shift }
sub request { return 1 }
sub close { return 1 }

package main;
my $value = shift @ARGV;  # runtime-unknown: only the ref() guard can refine the type
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
