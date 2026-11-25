package FAG::MooBuilderHook;
use Moo;
has 'value' => ( is => 'lazy', builder => '_build_value' );
1;
