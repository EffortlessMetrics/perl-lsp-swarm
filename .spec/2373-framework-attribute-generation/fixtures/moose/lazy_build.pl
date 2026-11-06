package FAG::MooseLazyBuild;
use Moose;
has 'value' => ( isa => 'Str', lazy_build => 1 );
1;
