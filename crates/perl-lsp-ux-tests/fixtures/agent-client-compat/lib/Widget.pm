package Widget;

use strict;
use warnings;

sub new {
    my ($class, $name) = @_;
    return bless { name => $name }, $class;
}

sub greet {
    my ($self) = @_;
    return "Hello, " . $self->{name};
}

1;
