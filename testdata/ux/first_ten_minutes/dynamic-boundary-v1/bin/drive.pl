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

# A genuine string-eval boundary: the expression text is compiled at runtime,
# and a failure must produce a bounded, explained fallback instead of dying
# uncaught or reporting a bare undef.
my $expression = '$dispatch->invoke(q{status})';
my $string_eval = eval $expression;
if ( defined $string_eval ) {
    print "string eval result: $string_eval\n";
}
else {
    my $reason = length $@ ? $@ : "string eval returned undef without raising\n";
    print "string eval fallback: $reason";
}
