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
sub regexparen ($r = /a(b/) { }
sub regexclose ($r = /a)b/) { }
sub regexclass ($r = /[)]/) { }
sub regexqr ($r = qr/a(b/) { }
sub regexqrbrace ($r = qr{ab)c}) { }
sub regexqrclass ($r = qr/[)]/) { }
sub regexmempty ($r = m//) { }
sub regexmparen ($r = m/a(b/) { }
sub regexsub ($r = s/a(b/c/) { }
sub regexattr ($r = /a(b/) : lvalue { }
sub divdefault ($x = $a / 2) { }
sub divnospace ($x = $a/$b) { }
sub nestedparen ($x = foo(1, (2))) { }
sub afterregex ($y) { }
sub grouped ([$@%&*]) { }
sub myref (\[$@%&*]) { }
sub nestedattr :attr(foo(1) :other) { }
sub nestedattr2 :attr(foo(1, bar(2)) :other) :lvalue { }
my $hsub = $h{sub};
