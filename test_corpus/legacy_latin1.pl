# Legacy Perl script with Latin-1 encoded comments
# Author: José López
# Beschreibung: Datei mit Sonderzeichen (Umlautä, ü, ö)

use strict;
use warnings;

my $name = "café";
my $greeting = "Schönes Guten Tag";

sub hello {
    my ($who) = @_;
    # Grüßt den Benutzer
    print "Hallo, $who!\n";
}

hello($name);
