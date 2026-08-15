use strict;
# gap-matrix: completion_constructor_assignment_control completion_typed_accessor_return
# gap-matrix: none accessor_return_not_promoted
use warnings;

package Example::Database;
sub new { return bless {}, shift }
sub connect { return 1 }

package Example::MooConfig;
use Moo;
has db => (is => 'ro', isa => 'Example::Database');
sub probe { my ($self) = @_; $self->db->connect; }
