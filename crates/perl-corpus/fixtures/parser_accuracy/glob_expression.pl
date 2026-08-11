package Accuracy::GlobExpression;

sub collect {
    my $pattern = "*.pl";
    my @files = glob($pattern);
    my @more = <$pattern>;
    return (@files, @more);
}

1;
