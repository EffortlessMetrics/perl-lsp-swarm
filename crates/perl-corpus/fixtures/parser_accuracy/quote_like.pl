package Accuracy::QuoteLike;

sub quote {
    my $message = q{hello};
    $message =~ s/hello/hello world/g;
    $message =~ tr/a-z/A-Z/;
    return qq{$message};
}

1;
