use strict;
use warnings;

package App::Vars;

sub show {
    my ($self, $name) = @_;
    return $self->{$name};
}

1;
