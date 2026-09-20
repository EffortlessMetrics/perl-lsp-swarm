package External::Client;
# gap-matrix: completion_method_return_chain_external_dependency
# gap-matrix: external_source_not_admitted
use strict;
use warnings;
use Example::Response;

sub new { return bless {}, shift }
sub response { return Example::Response->new }

1;
