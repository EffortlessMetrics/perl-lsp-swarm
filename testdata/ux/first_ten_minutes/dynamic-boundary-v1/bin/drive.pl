#!/usr/bin/perl
use strict;
use warnings;
use lib 'lib';

use Dispatch;

my $dispatch = Dispatch->new;

print $dispatch->status, "\n";
print $dispatch->invoke('status'), "\n";
print defined $dispatch->missing ? 'defined' : 'undef', "\n";

my $action = 'status';
my $result = eval { $dispatch->$action };
print "eval result: ", ( defined $result ? $result : 'undef' ), "\n";
