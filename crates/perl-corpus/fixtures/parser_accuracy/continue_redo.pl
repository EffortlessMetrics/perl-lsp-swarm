package Accuracy::ContinueRedo;

sub run {
    my $count = 0;
    OUTER: while ($count < 3) {
        $count++;
        redo OUTER if $count == 1;
        next OUTER if $count == 2;
        last OUTER;
    } continue {
        $count++;
    }
}

1;
