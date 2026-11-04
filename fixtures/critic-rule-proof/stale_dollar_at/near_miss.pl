use strict;
use warnings;
{
    local $@;
    eval { risky_call(); };
    if ($@) { warn $@; }
}
