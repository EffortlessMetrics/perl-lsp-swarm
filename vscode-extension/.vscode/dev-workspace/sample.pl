#!/usr/bin/env perl
# Bounded development workspace fixture for the Extension Development Host
# launches (#9851). F5 in vscode-extension/ opens THIS folder, never an
# arbitrary user workspace, so activation has one minimal Perl file to serve.
use strict;
use warnings;

print "perl-lsp dev workspace is alive\n";
