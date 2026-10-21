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
