use strict;
use warnings;
use FindBin qw($Bin);
use File::Spec;
use Config;
use Digest::SHA qw(sha256_hex);
use IPC::Open3;
use Symbol qw(gensym);
use Test::More tests => 12;

# Explicit offline oracle fixture. Never part of editor activation.
# Driver tooling (hashing, subprocess capture, TAP) is harness, not workload
# semantics claimed for the subject program.

# Hang containment for these trusted fixtures; not a semantic step budget.
if ($^O ne 'MSWin32') {
    $SIG{ALRM} = sub { BAIL_OUT('runtime-workload fixture exceeded 60 seconds') };
    alarm 60;
}
else {
    diag('no alarm deadline on MSWin32; trusted fixture only');
}

my $workload = File::Spec->catfile($Bin, 'spectralnorm.perl');

# Pinned extraction hash from PROVENANCE.md. Any byte change fails here.
my $pinned_sha = 'c7ad6bce4e8d44c5dd5892de3e480005d09f4d1eb54a3c3fd125e27facfdbc51';

sub slurp_raw {
    my ($path) = @_;
    open my $fh, '<:raw', $path or BAIL_OUT("open $path: $!");
    my $bytes = do { local $/; <$fh> };
    close $fh;
    return $bytes;
}

is(sha256_hex(slurp_raw($workload)), $pinned_sha, 'workload bytes match pinned extraction');

my $prov = slurp_raw(File::Spec->catfile($Bin, 'PROVENANCE.md'));
# Labeled fields, not bare tokens: each pinned value must appear on its own
# label line, so deleting or unlabeling a field fails the gate.
my @fields = (
    ['- Repository:', 'salsa.debian.org/benchmarksgame-team/benchmarksgame'],
    ['- Commit:', '40296663ed350d5fe4a6ab5e367bab61cb77c219'],
    ['- File:', 'public/program/spectralnorm-perl-1.html'],
    ['- Blob:', 'f41b3d7c06ddc809ee8e8b7c16fdcc7f377f7aa5'],
    ['- Byte range:', '1904-7939'],
    ['- Contributors', 'Andrew Rodland'],
    ['- Grant:', 'Revised BSD'],
    ['- sha256 of', $pinned_sha],
);
my @missing;
for my $field (@fields) {
    my ($label, $token) = @$field;
    my $found = grep { index($_, $label) == 0 && index($_, $token) >= 0 } split /\n/, $prov;
    push @missing, $label unless $found;
}
is_deeply(\@missing, [], 'provenance carries labeled identity fields with pinned values');

my $license = slurp_raw(File::Spec->catfile($Bin, 'LICENSE-benchmarksgame'));
ok($license =~ /Brent Fulgham/ && $license =~ /Redistribution and use in source and binary forms/,
    'BSD copyright notice and redistribution grant retained');

# Closed input world. The runner executes exactly the tuning keys below; adding,
# removing, or renaming a row changes behavior, and the fixed plan fails on an
# empty tuning set. Hold-out identities are reserved records, never executed.
my %tuning = (
    100 => "1.274219991\n",
    200 => "1.274223601\n",
);
my %holdout = (
    150 => 'held-out: identity fixed, expectation ungenerated, no tuning use',
    300 => 'held-out: identity fixed, expectation ungenerated, no tuning use',
);

is_deeply([sort { $a <=> $b } keys %tuning], [100, 200], 'tuning set is exactly the frozen inputs');
ok(!exists $tuning{999}, 'unknown input is outside the tuning set');
is_deeply([sort { $a <=> $b } keys %holdout], [150, 300], 'hold-out identities are exactly the reserved inputs');
is(scalar(grep { exists $tuning{$_} } keys %holdout), 0, 'hold-out inputs are not tuning inputs');

diag("stock Perl $^V; arch=$Config{archname}; ivsize=$Config{ivsize}; nvsize=$Config{nvsize}");

sub run_workload {
    my ($n) = @_;
    local %ENV = %ENV;
    delete @ENV{qw(PERL5OPT PERL5LIB PERLLIB)};
    $ENV{LC_ALL} = 'C';
    my $err = gensym();
    my $pid = open3(my $in, my $out, $err, $^X, $workload, $n);
    close $in;
    my $stdout = do { local $/; <$out> };
    my $stderr = do { local $/; <$err> };
    waitpid($pid, 0);
    # The numeric line is the observation; the platform line ending is channel.
    # Stock Perl on Windows translates \n to \r\n on output; compare on LF
    # rather than masking a real platform difference.
    $stdout =~ s/\r\n/\n/g if defined $stdout;
    $stderr =~ s/\r\n/\n/g if defined $stderr;
    return ($stdout // '', $stderr // '', $? >> 8);
}

my %observed_err;
for my $n (sort { $a <=> $b } keys %tuning) {
    my ($out, $err, $exit) = run_workload($n);
    is($exit, 0, "N=$n exits 0");
    is($out, $tuning{$n}, "N=$n output matches frozen observation");
    $observed_err{$n} = $err;
}
is_deeply([map { $observed_err{$_} } sort keys %observed_err], ['', ''], 'tuning runs produce no stderr');
if ($^O ne 'MSWin32') {
    alarm 0;
}
