# value_format_stdio_matrix.pl — exact public stdio proof fixture for #9590.
#
# The canary path arrives as ARGV[0]. Every user-code hook that ValueFormat
# formatting must never invoke (tied FETCH/STORE, overload stringification,
# object methods) appends one tagged line to that file; the proof requires
# the file to stay empty across every formatted inspection request.
#
# STOP1 (`$VF::stop1 = 1;`) and STOP2 (`$VF::stop2 = 1;`) are the two proof
# breakpoints: every value class is initialized before STOP1, and `$later`
# is fresh state that only exists at STOP2. The stop markers are package
# variables on purpose so they add no rows to the lexical pad dump.
#
# The lexical set is deliberately minimal: the locals dump is captured under
# a bounded acquisition window, and slow hosts (Windows-local pipes) need the
# dump to finish well inside that budget. Every #9590 value class keeps at
# least one row.
use strict;
use warnings;
# The proof-stop markers and the method-call subject are package variables
# that are each assigned exactly once; without this their "used only once"
# warnings would surface as debugger output and disturb the session.
no warnings 'once';

Canary::init(shift @ARGV);

my $pos   = 255;                       # positive integer
my $neg   = -42;                       # negative integer (sign-magnitude hex)
my $i_max = 9223372036854775807;       # i64::MAX: full-width hex
my $i_min = -9223372036854775807 - 1;  # i64::MIN without NV overflow
my $zero  = 0;
my $float = 2.5;                       # NV: numeric-but-not-integer class
my $looks = '42';                      # numeric-looking string (boundary)
my $uni   = "caf\x{e9} \x{2603}";      # Unicode string
my $u     = undef;
my @arr   = (10, 20, 30);
my %hash  = (a => 1, b => 'x');
my $ref   = \$pos;
my $over  = VF::Over->new;             # blessed + overloaded object
tie my $tied, 'VF::Tie';               # tied scalar (canary)

$VF::stop1 = 1;                        # STOP1
my $later = 4096;                      # fresh value proven at STOP2
$VF::stop2 = 1;                        # STOP2

print "done\n";

package Canary;

# Package variable on purpose: a file-scope `my` here would land in main's
# lexical pad and add a row to every locals dump.
our $canary_file;

sub init {
    my ($path) = @_;
    $canary_file = $path;
    open my $fh, '>', $canary_file or die "cannot truncate canary $canary_file: $!";
    close $fh;
    return;
}

sub hit {
    my ($tag) = @_;
    open my $fh, '>>', $canary_file or return;
    print {$fh} "$tag\n";
    close $fh;
    return;
}

package VF::Tie;

sub TIESCALAR {
    my ($class) = @_;
    return bless {}, $class;
}

sub FETCH {
    my ($self) = @_;
    Canary::hit('tie_FETCH');
    return 7;
}

sub STORE {
    my ($self, $value) = @_;
    Canary::hit('tie_STORE');
    return $value;
}

package VF::Over;

use overload
    '""' => sub { my ($self) = @_; Canary::hit('overload_stringify'); return 'OVERLOADED' },
    fallback => 1;

sub new {
    my ($class) = @_;
    return bless { inner => 5 }, $class;
}
