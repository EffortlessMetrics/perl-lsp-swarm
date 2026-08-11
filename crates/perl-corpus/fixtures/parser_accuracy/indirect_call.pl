package Accuracy::IndirectCall;

sub move {
    my ($player, @steps) = @_;
    return ($player, @steps);
}

sub run {
    my $player = "Ada";
    my $fh = *STDOUT;

    move $player 10, 20;
    print $fh "ready\\n";
    new Accuracy::IndirectCall::Runner "Ada";

    if ($player) {
        move $player 1;
    }

    move($player, 30);
}

1;
