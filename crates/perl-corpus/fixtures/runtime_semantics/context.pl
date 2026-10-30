# Source subjects for #17532; no test framework or I/O.
package RuntimeCohort;

sub record_context {
    my $context = wantarray;
    $_[0] = !defined($context) ? 'void' : $context ? 'list' : 'scalar';
    return (11, 22, 33);
}

1;
