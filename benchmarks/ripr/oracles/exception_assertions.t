use Test::More;
use Test::Exception;
throws_ok { die "boom" } qr/boom/, 'throws_ok observes the throw';
dies_ok { die "boom" } 'dies_ok observes death';
lives_ok { 1 } 'lives_ok observes survival';
done_testing;
