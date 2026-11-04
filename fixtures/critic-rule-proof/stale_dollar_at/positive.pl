use strict;
use warnings;
eval { risky_call(); };
if ($@) { warn $@; }
