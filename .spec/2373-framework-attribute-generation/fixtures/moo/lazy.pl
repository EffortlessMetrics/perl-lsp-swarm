package FAG::MooLazy;
use Moo;
has 'payload' => ( is => 'lazy', isa => 'Str' );
1;
