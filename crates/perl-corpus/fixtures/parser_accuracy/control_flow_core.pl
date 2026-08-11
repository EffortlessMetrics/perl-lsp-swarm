package Accuracy::ControlFlow;

sub run {
    my $value = 1;
    if ($value) { return $value; }
    while ($value) { last; }
}
