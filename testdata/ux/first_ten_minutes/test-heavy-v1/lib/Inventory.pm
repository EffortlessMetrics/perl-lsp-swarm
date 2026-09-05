package Inventory;
use strict;
use warnings;

sub new {
    my ( $class, %arg ) = @_;
    return bless { stock => $arg{stock} // {} }, $class;
}

sub add {
    my ( $self, $sku, $count ) = @_;
    $self->{stock}{$sku} = ( $self->{stock}{$sku} // 0 ) + $count;
    return $self->{stock}{$sku};
}

sub count {
    my ( $self, $sku ) = @_;
    return $self->{stock}{$sku} // 0;
}

sub skus {
    my ($self) = @_;
    return sort keys %{ $self->{stock} };
}

1;
