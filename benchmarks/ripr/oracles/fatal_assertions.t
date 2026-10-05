use Test::More;
use Test::Fatal qw(exception dies_ok lives_ok);
my $caught = exception { die "boom" };
ok($caught, 'exception captured the throw');
dies_ok { die "again" } 'dies_ok observes the throw';
lives_ok { 1 } 'lives_ok observes survival';
done_testing;
