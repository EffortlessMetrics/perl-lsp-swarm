use strict;
use warnings;

package App::Worker;

sub run {
    my ($task) = @_;
    return $task->();
}

1;
