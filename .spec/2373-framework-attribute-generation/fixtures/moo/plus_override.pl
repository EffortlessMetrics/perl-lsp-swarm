package FAG::MooOverride;
use Moo;
has 'id' => ( is => 'ro', isa => 'Int' );
has '+id' => ( is => 'rw' );
1;
