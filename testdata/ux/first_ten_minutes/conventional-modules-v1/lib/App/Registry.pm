package App::Registry;
use strict;
use warnings;

sub new {
    my ($class) = @_;
    return bless { entries => {} }, $class;
}

sub register {
    my ($self, $name, $value) = @_;
    $self->{entries}{$name} = $value;
    return $self;
}

sub lookup {
    my ($self, $name) = @_;
    return $self->{entries}{$name};
}

sub names {
    my ($self) = @_;
    return sort keys %{$self->{entries}};
}

1;
