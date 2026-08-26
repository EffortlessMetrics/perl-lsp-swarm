# Reachability denominator subject W1: canonical workspace edge subjects.
# Declared by fixtures/analysis_reachability_denominator/manifest.json (#10998).
package WorkspaceEdges;
use strict;
use warnings;

use lib 'lib';
use Target;
use Collision;    # same-name control: second package spelling must not merge edges

sub direct_call {
    return Target::run(1);
}

sub qualified_call {
    return Target->new->measure(2);
}

sub coderef_edge {
    my $handler = \&Target::run;
    return $handler->(3);
}

sub static_method_edge {
    return Target->build(4);
}

sub dynamic_non_edge {
    my ($method) = @_;
    return Target->$method(5);    # dynamic method name: unsupported non-edge
}

sub same_name_collision_control {
    return Collision::run(6);     # must not inherit Target::run facts
}

1;
