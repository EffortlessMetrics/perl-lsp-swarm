package Dispatch;
use strict;
use warnings;

our $AUTOLOAD;

sub new {
    my ($class) = @_;
    return bless { handled => { status => sub { 'ok' } } }, $class;
}

sub AUTOLOAD {
    my ($self) = @_;
    my ($name) = $AUTOLOAD =~ /::([^:]+)$/;
    return if !defined $name || $name eq 'DESTROY';
    my $handler = $self->{handled}{$name};
    return $handler ? $handler->() : undef;
}

sub invoke {
    my ( $self, $name ) = @_;
    my $method = "Dispatch::$name";
    return $self->$method;
}

1;
