# café test fixture -- Latin-1 / ISO-8859-1 encoded
# The word "café" contains byte 0xE9 which is invalid UTF-8.
package LegacyCafe;

sub hello { return "bonjour"; }

1;
