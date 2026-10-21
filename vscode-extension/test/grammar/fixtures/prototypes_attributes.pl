sub reindex ($$) : lvalue { $_[0] }
sub empty () { }
sub slurpy ($;@) { }
sub codef (\@) { }
sub multi ($$) :lvalue:locked { }
sub witharg ($$) :prototype($$) { }
sub forward ($$);
sub signed ($x, $y) { return $x; }
my $pid = $$;
my $tern = $pid ? 1 : 2;
my %h = (sub => 1);
print "sub fake ($$) :lvalue";
# sub commented ($$) :lvalue
sub Foo::Bar ($$) : lvalue { }
sub Foo::Bar::baz : lvalue { }
sub ternary ($x = $a ? 1 : 2) { }
sub ternarybare ($x = $a ? 1 : FALLBACK) : lvalue { }
sub ternaryword ($x = $a ? $b : $c) : lvalue { }
sub nsdefault ($x = Foo::Bar->new) : lvalue { }
sub hashkey ($hash{key}) { }
sub hashdefault ($opt = { mode => 'fast' }) : lvalue { }
sub strdefault ($x = "a:b") { }
sub argsattr ($$) :attr(1,2) :lvalue { }
sub meth :method { }
sub labelled { BODY: goto BODY; }
