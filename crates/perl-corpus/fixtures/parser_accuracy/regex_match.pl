package Accuracy::Regex;

sub matches {
    my $value = shift;
    return $value =~ /foo\d+/;
}

sub does_not_match {
    my $value = shift;
    return $value !~ /bar/;
}

1;
