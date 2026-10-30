use Test::More;
is(1 + 1, 2, 'addition');
isnt(1, 2, 'inequality');
is_deeply([1, 2], [1, 2], 'deep structure');
done_testing;
