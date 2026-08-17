use strict;
use warnings;
use Test::More tests => 1;
use Greeting;

is Greeting::greet("Sublime"), "Hello, Sublime", "greets through configured customlib";
