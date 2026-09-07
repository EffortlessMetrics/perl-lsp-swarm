#!/usr/bin/perl
# Sparse skeleton extracted from Dancer2 (https://github.com/PerlDancer/Dancer2)
# Licensed under the Artistic License 2.0
# Trimmed pinned 2.x fixture (#13616). Proves activation/import and
# core-DSL-registry behavior ONLY: two package apps, a literal appname, a
# silent no-op tag, an `!params` exclusion, and keyword usage in source text.
# Route declarations and hooks appear as source text for the L3/L4 leaves;
# this fixture must never be cited as proof of Dancer2 2.x config, template,
# serializer, or plugin behavior.
use strict;
use warnings;
use Test::More;

{
    package MyApp2x;
    use Dancer2 appname => 'MyApp2xNamed', ':script';

    get '/' => sub { 'Hello' };
    post '/inbox' => sub { 'Queued' };
    prefix '/api' => sub {
        any '/stats' => sub { 'ok' };
    };
    hook before => sub { '...'; };
    hook on_hook_exception => sub { '...'; };
}

{
    package MyApp2xAPI;
    use Dancer2 '!params';

    any '/alt-stats' => sub { 'ok' };
}

my $app = MyApp2x->to_app;
ok $app, 'the 2.x skeleton application builds';

done_testing();
