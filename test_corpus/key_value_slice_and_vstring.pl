#!/usr/bin/perl
use strict;
use warnings;

# Corpus fixture for NodeKinds no other corpus file exercises:
#   KeyValueSlice - `%hash{...}` postfix subscript on a % variable
#   VString       - a `v65.66.67` version-string literal
#
# Each kind appears in two distinct parent contexts (declaration initializer
# and expression/call position) so the NodeKind coverage gates are satisfied
# without allowlisting.

package Corpus::Coverage::KeyValueVString;

sub sample {
    my %config = (host => 'db', port => 5432, user => 'svc');

    # KeyValueSlice under a VariableDeclaration initializer.
    my %picked = %config{qw(host port)};

    # KeyValueSlice as an expression statement.
    %config{qw(user)};

    # VString under a VariableDeclaration initializer.
    my $encoded = v65.66.67;

    # VString as a builtin-call argument.
    print v76.111.111;

    return (\%picked, $encoded);
}

1;
