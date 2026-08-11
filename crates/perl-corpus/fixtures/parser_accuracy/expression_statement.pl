package Accuracy::ExpressionStatement;

sub transform {
    my $value = -1;
    $value += 2;
    my @values = (1, 2, 3);
    my %map = (alpha => 1, beta => 2);
    my @selected = @values[0, 2];
    my @named = @map{qw(alpha beta)};
    my %pairs = %map{qw(alpha beta)};
    return $value ? @selected : @named;
}

1;
