package External::Client;
use strict;
use warnings;
use Example::Response;

sub new { return bless {}, shift }
sub response { return Example::Response->new }

1;
