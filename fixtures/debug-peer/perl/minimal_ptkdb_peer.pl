#!/usr/bin/env perl
#
# minimal_ptkdb_peer.pl -- two deliberately narrow Perl Debugger Peer
# Protocol adapters for perl-dap (perl-debug-peer-v1):
#
#   * executed directly: a synthetic reference peer used to inspect and test
#     the wire contract without Devel::ptkdb;
#   * loaded from .ptkdbrc: an experimental mirror adapter for an explicitly
#     marked, ptkdb-shaped reference harness.
#
# The loaded adapter is not a DAP implementation and does not accept editor
# control. It authenticates to perl-dap, reports debugger-console output, emits
# stopped events from the explicitly marked reference harness's
# set_file($path, $line) seam, and emits one terminated event during Perl
# teardown. Capabilities remain empty. It is not a stock ptkdb integration.
#
# Load it from .ptkdbrc with an absolute path:
#
#   do '/path/to/minimal_ptkdb_peer.pl'
#       or die $@ || $!;
#
# Without the PERL_DAP_PEER rendezvous variables it is a silent no-op, so the
# same .ptkdbrc remains usable outside perl-dap. Only core Perl modules are used.

use strict;
use warnings;

package PerlDAP::PtkdbMirror;

use Errno qw(EAGAIN EINTR EWOULDBLOCK);
use IO::Select;
use IO::Socket::INET;
use JSON::PP qw(decode_json encode_json);
use Time::HiRes qw(time);

use constant PROTOCOL_VERSION     => 'perl-debug-peer-v1';
use constant REFERENCE_PTKDB_VERSION => '1.1091';
use constant REFERENCE_PTKDB_SOURCE  => 'CPAN:AEPAGE/Devel-ptkdb-1.1091';
use constant REFERENCE_PTKDB_MODULE_SHA256 => '2da4a792a732c134f8f4fa3b6b482da9e5df8dec8cd7ae424ad3b6e06c0bceab';
use constant REFERENCE_PTKDB_DIST_SHA256 => '889bfc25d107f46718963023cc9662d3d779896a48d729d0327beec0502c226e';
use constant MAX_HEADER_BYTES     => 8 * 1024;
use constant MAX_BODY_BYTES       => 8 * 1024 * 1024;
use constant CONNECT_TIMEOUT      => 2;
use constant HANDSHAKE_TIMEOUT    => 2;
use constant EVENT_WRITE_TIMEOUT  => 0.25;

our ($ACTIVE, $INSTALLED, $ORIGINAL_SET_FILE, $TERMINATED);
$INSTALLED  //= 0;
$TERMINATED //= 0;

sub _diagnostic {
    my ($message) = @_;
    print STDERR "minimal_ptkdb_peer.pl: $message\n";
}

sub _next_seq {
    my ($state) = @_;
    return ++$state->{seq};
}

sub _set_nonblocking {
    my ($socket) = @_;
    return 0 unless $socket->can('blocking');
    return eval { $socket->blocking(0); 1 } ? 1 : 0;
}

sub _write_all {
    my ($state, $bytes, $timeout) = @_;
    my $socket = $state->{socket};
    my $offset = 0;
    my $deadline = time() + $timeout;
    my $select = IO::Select->new($socket);

    # A host disconnect is an ordinary end to the mirror transport, not a
    # reason to terminate the debuggee. Ignore SIGPIPE only for the bounded
    # write; syswrite then reports EPIPE and the caller closes this peer state.
    local $SIG{PIPE} = 'IGNORE';

    while ($offset < length($bytes)) {
        my $remaining = $deadline - time();
        return (0, 'write timed out') if $remaining <= 0;
        return (0, 'socket was not writable before the deadline')
            unless $select->can_write($remaining);

        my $written = syswrite($socket, $bytes, length($bytes) - $offset, $offset);
        if (!defined $written) {
            next if $! == EINTR || $! == EAGAIN || $! == EWOULDBLOCK;
            return (0, "write failed: $!");
        }
        return (0, 'socket closed during write') if $written == 0;
        $offset += $written;
    }

    return (1, undef);
}

sub _send_message {
    my ($state, $message, $timeout) = @_;
    my $body = encode_json($message);
    return _write_all(
        $state,
        'Content-Length: ' . length($body) . "\r\n\r\n" . $body,
        $timeout,
    );
}

sub _read_more {
    my ($state, $deadline) = @_;
    my $select = IO::Select->new($state->{socket});

    while (1) {
        my $remaining = $deadline - time();
        return (0, 'read timed out') if $remaining <= 0;
        return (0, 'read timed out') unless $select->can_read($remaining);

        my $chunk = '';
        my $read = sysread($state->{socket}, $chunk, 4096);
        if (!defined $read) {
            next if $! == EINTR || $! == EAGAIN || $! == EWOULDBLOCK;
            return (0, "read failed: $!");
        }
        return (0, 'connection closed') if $read == 0;

        $state->{read_buffer} .= $chunk;
        return (1, undef);
    }
}

sub _read_message {
    my ($state, $timeout) = @_;
    my $deadline = time() + $timeout;

    while (1) {
        my $header_end = index($state->{read_buffer}, "\r\n\r\n");
        if ($header_end >= 0) {
            return (undef, 'frame header exceeds limit')
                if $header_end + 4 > MAX_HEADER_BYTES;

            my $header = substr($state->{read_buffer}, 0, $header_end + 4);
            my @lengths = $header =~ /^Content-Length:\s*(\d+)\s*\r$/gim;
            return (undef, 'frame must contain exactly one Content-Length header')
                unless @lengths == 1;
            my $body_length = 0 + $lengths[0];
            return (undef, 'frame body exceeds limit') if $body_length > MAX_BODY_BYTES;

            my $frame_length = $header_end + 4 + $body_length;
            if (length($state->{read_buffer}) >= $frame_length) {
                my $body = substr($state->{read_buffer}, $header_end + 4, $body_length);
                substr($state->{read_buffer}, 0, $frame_length, '');
                my $message = eval { decode_json($body) };
                return (undef, "invalid JSON body: $@") if $@;
                return ($message, undef);
            }
        } elsif (length($state->{read_buffer}) > MAX_HEADER_BYTES) {
            return (undef, 'frame header exceeds limit');
        }

        my ($ok, $error) = _read_more($state, $deadline);
        return (undef, $error) unless $ok;
    }
}

sub _validate_hello_response {
    my ($response, $hello_seq) = @_;
    return (0, 'peer/hello response must be a JSON object')
        unless ref($response) eq 'HASH';
    return (0, 'peer/hello response has an invalid type')
        unless defined $response->{type} && !ref($response->{type})
            && $response->{type} eq 'response';
    return (0, 'peer/hello response has an invalid request sequence')
        unless defined $response->{requestSeq} && !ref($response->{requestSeq})
            && $response->{requestSeq} =~ /\A\d+\z/
            && $response->{requestSeq} == $hello_seq;
    return (0, 'peer/hello response has an invalid command')
        unless defined $response->{command} && !ref($response->{command})
            && $response->{command} eq 'peer/hello';
    my $success = $response->{success};
    return (0, 'peer/hello response has an invalid success flag')
        unless defined $success
            && ref($success) eq 'JSON::PP::Boolean'
            && ("$success" eq '0' || "$success" eq '1');
    if (!$response->{success}) {
        return (0, 'peer/hello rejection has no string message')
            if defined $response->{message} && ref($response->{message});
        return (1, undef);
    }
    return (0, 'successful peer/hello response must contain an object body')
        unless ref($response->{body}) eq 'HASH';
    return (0, 'successful peer/hello response must contain a sessionId')
        unless defined $response->{body}{sessionId}
            && !ref($response->{body}{sessionId})
            && length $response->{body}{sessionId};
    return (1, undef);
}

sub _parse_rendezvous {
    my (%options) = @_;
    my $require_token = $options{require_token};

    my $address = $ENV{PERL_DAP_PEER};
    my $token = $ENV{PERL_DAP_PEER_TOKEN};
    my $mode = $ENV{PERL_DAP_PEER_MODE} // '';

    return (undef, 'PERL_DAP_PEER not set') unless defined $address && length $address;
    return (undef, "PERL_DAP_PEER_MODE='$mode' is not 'mirror'") unless $mode eq 'mirror';

    my ($host, $port) = $address =~ /\A(127(?:\.\d{1,3}){3}):(\d{1,5})\z/;
    return (undef, "malformed or non-loopback PERL_DAP_PEER '$address'")
        unless defined $host && defined $port;
    my @octets = split /\./, $host;
    return (undef, "malformed or non-loopback PERL_DAP_PEER '$address'")
        unless @octets == 4 && !grep { $_ > 255 } @octets;
    return (undef, "invalid PERL_DAP_PEER port '$port'")
        unless $port > 0 && $port <= 65_535;

    if ($require_token) {
        return (undef, 'PERL_DAP_PEER_TOKEN is required for the reference mirror adapter')
            unless defined $token && $token =~ /\A[0-9A-Fa-f]{32}\z/;
    }

    return ({ address => $address, host => $host, port => 0 + $port, token => $token }, undef);
}

sub _connect_and_handshake {
    my (%options) = @_;
    my ($rendezvous, $error) = _parse_rendezvous(require_token => $options{require_token});
    return (undef, $error) unless $rendezvous;

    my $socket = IO::Socket::INET->new(
        PeerAddr => $rendezvous->{host},
        PeerPort => $rendezvous->{port},
        Proto    => 'tcp',
        Timeout  => CONNECT_TIMEOUT,
    );
    return (undef, "cannot connect to $rendezvous->{address}: $!") unless $socket;
    $socket->autoflush(1);
    unless (_set_nonblocking($socket)) {
        close($socket);
        return (undef, 'cannot enable nonblocking peer I/O');
    }

    my $state = {
        socket      => $socket,
        seq         => 0,
        read_buffer => '',
        closed      => 0,
        nonblocking => 1,
    };

    my $hello_seq = _next_seq($state);
    my %arguments = (
        peer            => $options{peer},
        peerVersion     => $options{peer_version},
        protocolVersion => PROTOCOL_VERSION,
        capabilities    => {},
    );
    $arguments{token} = $rendezvous->{token} if defined $rendezvous->{token};

    my ($sent, $send_error) = _send_message(
        $state,
        {
            type      => 'request',
            seq       => $hello_seq,
            command   => 'peer/hello',
            arguments => \%arguments,
        },
        HANDSHAKE_TIMEOUT,
    );
    unless ($sent) {
        close($socket);
        return (undef, "peer/hello write failed: $send_error");
    }

    my $deadline = time() + HANDSHAKE_TIMEOUT;
    while (time() < $deadline) {
        my ($response, $read_error) = _read_message($state, $deadline - time());
        unless (defined $response) {
            close($socket);
            my $failure = defined $read_error
                ? $read_error
                : 'peer/hello response must be a JSON value';
            return (undef, "peer/hello response failed: $failure");
        }
        my ($valid, $validation_error) = _validate_hello_response($response, $hello_seq);
        unless ($valid) {
            close($socket);
            return (undef, "invalid peer/hello response: $validation_error");
        }
        unless ($response->{success}) {
            my $why = $response->{message} // 'unknown reason';
            close($socket);
            return (undef, "peer/hello rejected by host: $why");
        }
        $state->{session_id} = $response->{body}{sessionId} // '(no sessionId)';
        return ($state, undef);
    }

    close($socket);
    return (undef, 'peer/hello response timed out');
}

sub _emit_event {
    my ($state, $event, $body) = @_;
    return 0 unless $state && !$state->{closed};
    my ($ok) = _send_message(
        $state,
        {
            type  => 'event',
            seq   => _next_seq($state),
            event => $event,
            body  => $body,
        },
        EVENT_WRITE_TIMEOUT,
    );
    if (!$ok) {
        $state->{closed} = 1;
        close($state->{socket});
    }
    return $ok;
}

sub _emit_output {
    my ($state, $category, $output) = @_;
    return _emit_event($state, 'debugger/output', { category => $category, output => $output });
}

sub _emit_stopped {
    my ($state, $path, $line) = @_;
    my %body = (reason => 'pause', threadId => 1);
    $body{source} = { path => "$path" } if defined $path && length "$path";
    $body{line} = 0 + $line if defined $line && $line =~ /\A\d+\z/ && $line > 0;
    return _emit_event($state, 'debugger/stopped', \%body);
}

sub _emit_terminated {
    my ($state) = @_;
    return 0 if $TERMINATED;
    $TERMINATED = 1;
    my $ok = _emit_event($state, 'debugger/terminated', {});
    if ($state && $state->{socket}) {
        close($state->{socket});
        $state->{closed} = 1;
    }
    return $ok;
}

sub _ptkdb_version {
    return $Devel::ptkdb::VERSION if defined $Devel::ptkdb::VERSION;
    return $DB::VERSION if defined $DB::VERSION;
    return undef;
}

sub _ptkdb_source {
    return $Devel::ptkdb::PERL_DAP_MIRROR_SOURCE
        if defined $Devel::ptkdb::PERL_DAP_MIRROR_SOURCE;
    return undef;
}

sub _ptkdb_declared_module_digest {
    return $Devel::ptkdb::PERL_DAP_MIRROR_SHA256
        if defined $Devel::ptkdb::PERL_DAP_MIRROR_SHA256;
    return undef;
}

sub _ptkdb_declared_dist_digest {
    return $Devel::ptkdb::PERL_DAP_MIRROR_DIST_SHA256
        if defined $Devel::ptkdb::PERL_DAP_MIRROR_DIST_SHA256;
    return undef;
}

sub _is_sha256 {
    my ($value) = @_;
    return defined $value && "$value" =~ /\A[0-9a-f]{64}\z/;
}

sub _check_ptkdb_provenance {
    my $loaded_path = $INC{'Devel/ptkdb.pm'};
    my $source = _ptkdb_source();
    my $declared_module_digest = _ptkdb_declared_module_digest();
    my $declared_dist_digest = _ptkdb_declared_dist_digest();

    return (0, 'ptkdb source and distribution provenance fields are required')
        unless defined $source && defined $declared_module_digest
            && defined $declared_dist_digest;
    return (0, 'ptkdb module digest marker is not a lowercase SHA-256 digest')
        unless _is_sha256($declared_module_digest);
    return (0, 'ptkdb distribution digest marker is not a lowercase SHA-256 digest')
        unless _is_sha256($declared_dist_digest);
    return (0, 'ptkdb source identity does not match the pinned CPAN artifact')
        unless "$source" eq REFERENCE_PTKDB_SOURCE;
    return (0, 'ptkdb distribution digest does not match the pinned CPAN artifact')
        unless "$declared_dist_digest" eq REFERENCE_PTKDB_DIST_SHA256;

    if (exists $INC{'Devel/ptkdb.pm'}) {
        return (0, 'loaded Devel/ptkdb.pm bytes cannot be bound to this provenance check; refusing loaded-module activation');
    }

    # The headless harness has no installed module file. Its explicit contract
    # carries the same immutable module and distribution identities; this remains
    # harness proof only.
    return (0, 'reference harness provenance does not match the pinned CPAN artifact')
        unless "$source" eq REFERENCE_PTKDB_SOURCE
            && "$declared_module_digest" eq REFERENCE_PTKDB_MODULE_SHA256;
    return (1, undef);
}

sub _after_set_file {
    my ($state, $caller_sub, $path, $line) = @_;
    return unless $caller_sub eq 'DB::DB';
    _emit_stopped($state, $path, $line);
}

sub install_ptkdb_mirror {
    return 1 if $INSTALLED;

    my $opted_in = grep { defined $ENV{$_} && length $ENV{$_} }
        qw(PERL_DAP_PEER PERL_DAP_PEER_TOKEN PERL_DAP_PEER_MODE);
    return 1 unless $opted_in;

    my $version = _ptkdb_version();
    unless (defined $version && "$version" eq REFERENCE_PTKDB_VERSION) {
        _diagnostic(
            'reference mirror adapter requires Devel::ptkdb '
                . REFERENCE_PTKDB_VERSION
                . '; observed '
                . (defined $version ? $version : 'no loaded ptkdb version')
                . ' -- leaving ptkdb untouched'
        );
        return 1;
    }
    my ($proven, $provenance_error) = _check_ptkdb_provenance();
    unless ($proven) {
        _diagnostic("reference mirror adapter provenance check failed: $provenance_error -- leaving ptkdb untouched");
        return 1;
    }
    unless (defined &Devel::ptkdb::set_file) {
        _diagnostic('reference ptkdb set_file seam is unavailable -- leaving ptkdb untouched');
        return 1;
    }

    my ($state, $error) = _connect_and_handshake(
        require_token => 1,
        peer          => 'Devel::ptkdb reference mirror adapter',
        peer_version  => REFERENCE_PTKDB_VERSION,
    );
    unless ($state) {
        _diagnostic("reference mirror adapter disabled: $error");
        return 1;
    }

    $ACTIVE = $state;
    $ORIGINAL_SET_FILE = \&Devel::ptkdb::set_file;

    {
        no warnings 'redefine';
        *Devel::ptkdb::set_file = sub {
            my @arguments = @_;
            my $caller_sub = (caller(1))[3] // '';
            my $wantarray = wantarray;

            if (!defined $wantarray) {
                $ORIGINAL_SET_FILE->(@arguments);
                _after_set_file($ACTIVE, $caller_sub, $arguments[1], $arguments[2]);
                return;
            }
            if ($wantarray) {
                my @result = $ORIGINAL_SET_FILE->(@arguments);
                _after_set_file($ACTIVE, $caller_sub, $arguments[1], $arguments[2]);
                return @result;
            }

            my $result = $ORIGINAL_SET_FILE->(@arguments);
            _after_set_file($ACTIVE, $caller_sub, $arguments[1], $arguments[2]);
            return $result;
        };
    }

    $INSTALLED = 1;
    _emit_output(
        $ACTIVE,
        'console',
        'Devel::ptkdb '
            . REFERENCE_PTKDB_VERSION
            . " reference mirror connected; reporting console/stopped/terminated only\n"
    );
    return 1;
}

sub run_reference_peer {
    my ($state, $error) = _connect_and_handshake(
        require_token => 0,
        peer          => 'minimal_ptkdb_peer.pl (reference)',
        peer_version  => '0.1',
    );
    unless ($state) {
        _diagnostic("reference peer disabled: $error");
        return 0;
    }

    _diagnostic("handshake ok, session=$state->{session_id}");
    _emit_output($state, 'stdout', "minimal_ptkdb_peer.pl: reference peer connected\n");
    _emit_event(
        $state,
        'debugger/stopped',
        {
            reason   => 'entry',
            threadId => 1,
            source   => { path => $0 },
            line     => 1,
        },
    );

    while (!$state->{closed}) {
        my ($message, $read_error) = _read_message($state, 1);
        if (!$message) {
            next if $read_error eq 'read timed out';
            last;
        }
        unless (ref($message) eq 'HASH') {
            _diagnostic('post-handshake frame must be a JSON object; closing peer session');
            last;
        }
        next unless ($message->{type} // '') eq 'request';
        my $command = $message->{command} // '';
        next unless $command eq 'peer/goodbye' || $command eq 'debugger/disconnect';
        _send_message(
            $state,
            {
                type       => 'response',
                seq        => _next_seq($state),
                requestSeq => $message->{seq},
                success    => JSON::PP::true,
                command    => $command,
            },
            EVENT_WRITE_TIMEOUT,
        );
        last;
    }

    close($state->{socket});
    $state->{closed} = 1;
    return 0;
}

END {
    _emit_terminated($ACTIVE) if $ACTIVE && $INSTALLED;
}

package main;

if (defined caller) {
    PerlDAP::PtkdbMirror::install_ptkdb_mirror();
} else {
    exit PerlDAP::PtkdbMirror::run_reference_peer();
}

1;
