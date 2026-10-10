use strict;
use warnings;
use FindBin qw($Bin);
use File::Spec;
use Config;
use Test::More tests => 50;

# Linux proof cell: bound regressions such as a lost loop update.
$SIG{ALRM} = sub { BAIL_OUT('runtime-semantic fixture exceeded 10 seconds') };
alarm 10;

# Explicit offline oracle fixture. Never part of editor activation.
# Test-driver modules and TAP are not semantics claimed for the source subjects.
my @source_files = qw(values.pl calls.pl control.pl places.pl context.pl unwind.pl);
my (@warnings, $subject_stdout, $subject_stderr);
$subject_stdout = '';
$subject_stderr = '';
{
    open my $out, '>', \$subject_stdout or BAIL_OUT("capture subject stdout: $!");
    open my $err, '>', \$subject_stderr or BAIL_OUT("capture subject stderr: $!");
    local *STDOUT = $out;
    local *STDERR = $err;
    local $SIG{__WARN__} = sub { push @warnings, @_ };
    for my $name (@source_files) {
        my $source = File::Spec->catfile($Bin, $name);
        my $loaded = do $source;
        BAIL_OUT("load $source: " . ($@ || $! || 'false source result')) unless $loaded;
    }
}
diag("stock Perl $^V; arch=$Config{archname}; ivsize=$Config{ivsize}; nvsize=$Config{nvsize}");

# Observe unexpected warnings/output without mixing them with the driver's TAP.
sub observed (&) {
    my ($operation) = @_;
    open my $out, '>>', \$subject_stdout or BAIL_OUT("capture subject stdout: $!");
    open my $err, '>>', \$subject_stderr or BAIL_OUT("capture subject stderr: $!");
    local *STDOUT = $out;
    local *STDERR = $err;
    local $SIG{__WARN__} = sub { push @warnings, @_ };
    # Preserve caller scalar/list context. Void-context calls are explicit below.
    return $operation->();
}

is(observed { RuntimeCohort::arithmetic(2, 3, 4) }, 17, 'arithmetic operand order');
is(observed { RuntimeCohort::arithmetic(-2, 3, 4) }, 1, 'negative arithmetic input');
is(observed { RuntimeCohort::arithmetic(5, 0, 0) }, 0, 'zero arithmetic input');
is_deeply([observed { RuntimeCohort::comparisons('09', '9') }], [1, 0], 'numeric equality is not string equality');
is_deeply([observed { RuntimeCohort::comparisons('12', '12') }], [1, 1], 'equal numeric strings');
is_deeply([observed { RuntimeCohort::comparisons('12', '13') }], [0, 0], 'unequal numeric strings');
is_deeply([observed { RuntimeCohort::truth_and_definedness(undef) }], [0, 0], 'undef is neither defined nor true');
is_deeply([observed { RuntimeCohort::truth_and_definedness('') }], [1, 0], 'empty string is defined but false');
is_deeply([observed { RuntimeCohort::truth_and_definedness('0') }], [1, 0], 'string zero is defined but false');
is_deeply([observed { RuntimeCohort::truth_and_definedness('00') }], [1, 1], 'double zero is true');
is_deeply([observed { RuntimeCohort::truth_and_definedness(0) }], [1, 0], 'numeric zero is defined but false');
is_deeply([observed { RuntimeCohort::truth_and_definedness(-1) }], [1, 1], 'negative number is true');
is(observed { RuntimeCohort::default_value(undef) }, 'fallback', 'defined-or selects missing value');
is(observed { RuntimeCohort::default_value('0') }, '0', 'defined-or preserves false value');
is(observed { RuntimeCohort::default_value('') }, '', 'defined-or preserves empty value');

my $digits = '009';
is(observed { RuntimeCohort::increment_alias($digits) }, '010', 'string increment preserves leading zero');
is($digits, '010', 'increment writes through argument alias');
my $numeric_digits = '009';
is(observed { RuntimeCohort::numeric_then_increment($numeric_digits) }, 9, 'numeric use observes original numeric value');
is($numeric_digits, '10', 'numeric history changes later increment');
my $letters = 'a9';
is(observed { RuntimeCohort::increment_alias($letters) }, 'b0', 'string increment carries into letter');
my ($a, $b) = (4, 5);
is_deeply([observed { RuntimeCohort::update_pair($a, $b) }], [6, 15], 'distinct argument cells update separately');
is_deeply([$a, $b], [6, 15], 'distinct alias updates reach caller');
my $same = 4;
is_deeply([observed { RuntimeCohort::update_pair($same, $same) }], [18, 18], 'repeated arguments retain one cell');
is($same, 18, 'shared alias caller sees both sequential writes');
my $copied = 4;
is(observed { RuntimeCohort::update_copy($copied) }, 9, 'local copy can be changed');
is($copied, 4, 'changing copy does not change caller');
is(observed { RuntimeCohort::nested_call(3) }, 20, 'nested direct calls compose');
is(observed { RuntimeCohort::factorial(0) }, 1, 'recursive base case');
is(observed { RuntimeCohort::factorial(5) }, 120, 'recursive calls retain distinct lexical pads');
is(scalar(observed { RuntimeCohort::return_values() }), 33, 'scalar list expression returns last element, not count');
is_deeply([observed { RuntimeCohort::return_values() }], [11, 22, 33], 'list context retains all elements');
my $context = '';
my $scalar_result = observed { scalar RuntimeCohort::record_context($context) };
is_deeply([$context, $scalar_result], ['scalar', 33], 'scalar caller context reaches callee');
my @list_result = observed { RuntimeCohort::record_context($context) };
is_deeply([$context, @list_result], ['list', 11, 22, 33], 'list caller context reaches callee');
observed { RuntimeCohort::record_context($context); return; };
is($context, 'void', 'void caller context reaches callee');
is_deeply([observed { RuntimeCohort::short_circuit(0, 7) }], [0, 0], 'false AND skips right-side effect');
is_deeply([observed { RuntimeCohort::short_circuit(1, 7) }], [7, 1], 'true AND evaluates right side once');
is_deeply([observed { RuntimeCohort::select_arm(1) }], ['yes', 1], 'ternary executes only true arm');
is_deeply([observed { RuntimeCohort::select_arm(0) }], ['no', 1], 'ternary executes only false arm');
is_deeply([observed { RuntimeCohort::modify_place() }], [10, 4, 1], 'compound place index evaluated once');
is(observed { RuntimeCohort::sum_loop(0) }, 0, 'loop has zero-iteration path');
is(observed { RuntimeCohort::sum_loop(6) }, 15, 'loop carries sum and performs update');
is_deeply([observed { RuntimeCohort::next_last_continue() }], ['1cc3c', 4], 'next runs continue and last skips it');
is_deeply([observed { RuntimeCohort::redo_continue() }], ['bbcbc', 2], 'redo skips condition and continue');
is_deeply([observed { RuntimeCohort::foreach_alias() }], [2, 4, 6], 'foreach variable aliases elements');
my $written = 5;
is_deeply([observed { RuntimeCohort::effect_before_failure($written) }], [undef, 8, 'outer', "cohort-stop\n"], 'ordinary write survives exception while local restores');
is($written, 8, 'caller observes exactly one pre-failure effect');
is_deeply([observed { RuntimeCohort::nested_localization() }], ['middle', 'outer', "outer-stop\n"], 'nested local scopes restore at their own unwind boundaries');
{
    local $RuntimeCohort::Dynamic = 'caller';
    my $inside = observed { RuntimeCohort::return_from_local() };
    is_deeply([$inside, $RuntimeCohort::Dynamic], ['temporary', 'caller'], 'return also restores localization');
}
is_deeply(\@warnings, [], 'subjects produce no unexpected warnings');
is_deeply([$subject_stdout, $subject_stderr], ['', ''], 'subjects produce no output; TAP belongs to driver');
alarm 0;
