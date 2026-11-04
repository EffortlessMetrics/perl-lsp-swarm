use Test::More;
cmp_ok(10, '>', 5, 'ten exceeds five');
like("hello world", qr/world/, 'greeting matches');
unlike("hello", qr/bye/, 'no farewell');
isa_ok([], 'ARRAY', 'anonymous array');
can_ok('My::App', 'discount');
done_testing;
