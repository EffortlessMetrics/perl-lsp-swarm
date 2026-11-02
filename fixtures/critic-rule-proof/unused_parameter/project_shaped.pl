use strict;
use warnings;

package App::API;

sub handle {
    my ($self, $req) = @_;
    return $self->dispatch($req);
}

1;
