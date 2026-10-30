use strict;
use warnings;
## no critic native.common.stale_dollar_at -- legacy eval
eval { risky_call(); };
if ($@) { warn $@; }
