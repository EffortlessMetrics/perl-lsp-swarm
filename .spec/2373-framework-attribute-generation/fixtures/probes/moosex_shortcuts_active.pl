package FAG::MooseShortcutsPositive;
use Moose;
use MooseX::AttributeShortcuts;
has 'note' => ( is => 'rwp', isa => 'Str' );
1;
