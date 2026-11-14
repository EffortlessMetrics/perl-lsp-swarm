# stress: nested blocks and eval
{
    my $x = eval { 1 + 1 };
    if ($x) { $x++; }
}
