package FAG::MooDynamicReader;
use Moo;
has 'slot' => ( is => 'ro', reader => sub { 'dynamic' } );
1;
