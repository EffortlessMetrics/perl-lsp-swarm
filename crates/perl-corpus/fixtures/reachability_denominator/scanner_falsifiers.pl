# Reachability denominator subject A2: scanner falsifiers.
# Flow-like spellings inside non-code surfaces must never become flow facts.
# Declared by fixtures/analysis_reachability_denominator/manifest.json (#10998).
use strict;
use warnings;

my $template = <<'END_HEREDOC';
    sub phantom_heredoc_sub {
        die "not code\n";
        return 'never parsed as flow';
    }
END_HEREDOC

my $regex_control = qr/
    exit \s+ 0      # 'exit' inside an extended regex literal is text
    |
    return          # 'return' inside the same literal is text
/x;

my $string_control = "goto somewhere; exec('never');";

=pod

=head1 phantom_pod_section

sub phantom_pod_sub {
    return 'pod body is not executable flow';
    die "also not code\n";
}

=cut

__END__
sub phantom_end_sub {
    exit 1;
    print 'after __END__ is data, not flow';
}

print "entry reachable\n";
