use Test::More;
use Test::Warn;
warning_is { warn "careful" } "careful", 'warning_is matches text';
warning_like { warn "careful" } qr/care/, 'warning_like matches pattern';
warnings_are { warn "one"; warn "two" } ["one", "two"], 'warnings_are matches list';
done_testing;
