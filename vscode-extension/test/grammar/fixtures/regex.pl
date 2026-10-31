my $matched = $text =~ m/foo(bar)?/i;
(my $copy = $orig) =~ s/old/new/g;
$str =~ tr/a-z/A-Z/;
my $compiled = qr/\d+\.\d+/x;
if ($input =~ /^(\w+)\s*=\s*(.+)$/) {
    my ($key, $value) = ($1, $2);
}
my $size = -s $file;
if (-s $fh) { say "nonempty"; }
my %h = (x => 1, y => 2);
$h{s} = 1;
my %r = (s => 1, tr => 2, y => 3);
s/cat/dog/;
$_ =~ s/^\s+|\s+$//g;
s{old}{new}g;
s{a{b}c}{d}g;
s#foo#bar#g;
my $u = $text =~ s/x/y/gr;
my $q = $x / 2;
map {s/x/y/} @list;
