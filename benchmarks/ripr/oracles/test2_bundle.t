use Test2::V0;
is(2 * 3, 6, 'multiplication');
ok(1, 'truthy');
like("abc", qr/b/, 'contains b');
done_testing;
