use Test::More;
ok(1, 'first');
pass('second');
fail('labeled smoke-oracle call; fixtures never execute');
diag('diagnostic output, not an oracle');
note('quiet note, not an oracle');
done_testing;
