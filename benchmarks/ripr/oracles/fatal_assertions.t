use Test::More;
use Test::Fatal;
my $caught = exception { die "boom" };
ok($caught, 'exception captured the throw');
ok(dies { die "again" }, 'dies observes the throw');
ok(lives { 1 }, 'lives observes survival');
done_testing;
