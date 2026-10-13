#!/usr/bin/perl
use strict;
use warnings;
use Mojolicious::Lite;

get '/' => sub {
    my ($c) = @_;
    return $c->render( json => { status => 'ok' } );
};

get '/task/:id' => sub {
    my ($c) = @_;
    my $id = $c->stash('id');
    return $c->render( json => { id => $id } );
};

app->start;
