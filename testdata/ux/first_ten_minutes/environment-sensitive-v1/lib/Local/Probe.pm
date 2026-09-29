package Local::Probe;
use strict;
use warnings;
use Config;

sub site_lib {
    my ($class) = @_;
    return $Config{sitelib};
}

sub perl_version {
    my ($class) = @_;
    return $^V;
}

1;
