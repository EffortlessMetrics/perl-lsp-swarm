# Sparse skeleton extracted from Dancer2 (https://github.com/PerlDancer/Dancer2)
# Licensed under the Artistic License 2.0
# Original copyright: Alexis Sukrieh, Sawyer X and contributors
# Trimmed pinned 2.x fixture (#13616). Proves activation/import and
# core-DSL-registry behavior ONLY. This fixture must never be cited as proof
# of Dancer2 2.x config, template, serializer, or plugin behavior.
package Dancer2;
use strict;
use warnings;

our $VERSION = '2.0.1';

sub import {
    my ($class, @args) = @_;
    my ($caller, $script) = caller;

    my @final_args;
    my $clean_import;
    foreach my $arg (@args) {
        grep +($arg eq $_), qw<:script :syntax :tests>
          and next;
        if ($arg eq ':nopragmas') {
            $clean_import++;
            next;
        }
        if (substr($arg, 0, 1) eq '!') {
            push @final_args, $arg, 1;
        }
        else {
            push @final_args, $arg;
        }
    }

    $clean_import
      or $_->import::into($caller)
      for qw<strict warnings utf8>;

    scalar @final_args % 2
      and die q{parameters must be key/value pairs or '!keyword'};

    my %final_args = @final_args;
    my $appname = delete $final_args{appname};
    $appname ||= $caller;
    return $appname;
}

1;
