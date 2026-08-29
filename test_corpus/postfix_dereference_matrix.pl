#!/usr/bin/env perl
# Exact runtime corpus for Perl's non-interpolated postfix-dereference family.

use v5.24;
use strict;
use warnings;
use utf8;

my $scalar = 41;
my $sref = \$scalar;
my $scalar_value = $sref->$*;

my $aref = [10, 20, 30, 40];
my $last_index = $aref->$#*;
my @all_values = $aref->@*;
my @selected_values = $aref->@[0, 2];

my $href = { alpha => 1, beta => 2, '東京' => 3 };
my @selected_keys = $href->@{'alpha', 'beta'};
my %all_pairs = $href->%*;
my %selected_pairs = $href->%{qw(alpha beta)};

my $cref = sub { return 42 };
my $code_result = $cref->&*;

my $gref = \*STDOUT;
my $glob_name = *{$gref->**}{NAME};

my $object = { payload => $href };
my @chained = $object->{payload}->@{'東京', 'alpha'};

$aref->@[1, 3] = (21, 41);
$href->@{qw(alpha beta)} = (4, 5);

# Nearby generic forms: none of these may satisfy a postfix expectation.
my %control_hash = ( alpha => 1, beta => 2, '東京' => 3 );
my @ordinary_hash_slice = @control_hash{'alpha', 'beta'};
my %ordinary_kv_slice   = %control_hash{qw(alpha beta)};
my @prefix_hash_slice   = @$href{'alpha', 'beta'};
my %prefix_kv_slice     = %$href{qw(alpha beta)};
my $control_element     = $href->{alpha};

# Repeated marker: the same postfix text must bind twice, never once.
my @repeat_first  = $href->@{'beta', '東京'};
my @repeat_second = $href->@{'beta', '東京'};

die "scalar dereference failed" unless $scalar_value == 41;
die "last-index dereference failed" unless $last_index == 3;
die "full array dereference failed" unless @all_values == 4;
die "array slice dereference failed" unless "@selected_values" eq "10 30";
die "hash slice dereference failed" unless "@selected_keys" eq "1 2";
die "full hash dereference failed" unless $all_pairs{'東京'} == 3;
die "key/value slice dereference failed" unless $selected_pairs{beta} == 2;
die "code dereference failed" unless $code_result == 42;
die "glob dereference failed" unless defined $glob_name && $glob_name eq "STDOUT";
die "chained hash slice failed" unless "@chained" eq "3 1";
die "array slice lvalue failed" unless "@{$aref}[1, 3]" eq "21 41";
die "hash slice lvalue failed" unless "$href->{alpha} $href->{beta}" eq "4 5";
die "ordinary hash slice failed" unless "@ordinary_hash_slice" eq "1 2";
die "ordinary key/value slice failed" unless $ordinary_kv_slice{beta} == 2;
die "prefix hash slice failed" unless "@prefix_hash_slice" eq "4 5";
die "prefix key/value slice failed" unless $prefix_kv_slice{alpha} == 4;
die "control element failed" unless $control_element == 4;
die "repeated hash slice first failed" unless "@repeat_first" eq "5 3";
die "repeated hash slice second failed" unless "@repeat_second" eq "5 3";

print "postfix-dereference-matrix: ok\n";
