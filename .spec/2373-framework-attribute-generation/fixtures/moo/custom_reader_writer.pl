package FAG::MooCustomNames;
use Moo;
has 'title' => (
    is     => 'ro',
    reader => 'get_title',
    writer => 'set_title',
);
1;
