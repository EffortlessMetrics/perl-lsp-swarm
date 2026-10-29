use feature 'class';
class Widget {
    method render {
        my $self = shift;
        return $self->{html};
    }
}
1;
