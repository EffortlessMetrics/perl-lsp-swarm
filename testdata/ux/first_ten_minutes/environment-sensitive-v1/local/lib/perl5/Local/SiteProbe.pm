package Local::SiteProbe;
use strict;
use warnings;

sub vendor_marker {
    my ($class) = @_;
    return 'resolved through the vendored local/lib/perl5 include path';
}

1;
