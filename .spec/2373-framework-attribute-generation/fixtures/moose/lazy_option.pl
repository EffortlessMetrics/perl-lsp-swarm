package FAG::MooseLazy;
use Moose;
has 'payload' => ( is => 'ro', isa => 'Str', lazy => 1, builder => '_build_payload' );
1;
