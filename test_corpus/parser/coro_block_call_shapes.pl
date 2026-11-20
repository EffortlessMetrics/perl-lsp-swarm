#!/usr/bin/env perl
# Focused CPAN Coro block-call shapes promoted from
# test_corpus/real_world/async_event_patterns.pl (#8316).

use strict;
use warnings;

# Imported block calls
async { work() };
async { work($_[0]) } $arg;
Coro::async { work() };
async_pool { work() };

# Assignment from async block call (worker spawn pattern)
my $worker = async {
    work($i);
};

# Ordinary identifiers that must not become keywords
sub async { 1 }
sub coro { 1 }
package yield;
my %h = (cede => 1, schedule => 2);

# Malformed editing states (recovery fixtures)
# async {
# async { foo(
# Coro::async {
