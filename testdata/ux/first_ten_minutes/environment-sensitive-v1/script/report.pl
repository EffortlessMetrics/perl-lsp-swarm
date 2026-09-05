#!/usr/bin/perl
use strict;
use warnings;

use lib 'lib';
use lib 'local/lib/perl5';

use Local::Probe;
use Local::SiteProbe;

print "perl: ", Local::Probe->perl_version, "\n";
print "sitelib: ", Local::Probe->site_lib, "\n";
print "vendored: ", Local::SiteProbe->vendor_marker, "\n";
print "PERL5LIB: ", ( $ENV{PERL5LIB} // '(unset)' ), "\n";
