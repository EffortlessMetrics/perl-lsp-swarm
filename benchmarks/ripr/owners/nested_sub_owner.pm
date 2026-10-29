package Outer {
    our $FLAG = 1;
    sub inner {
        my $worked = 1;
        return $worked;
    }
}
1;
