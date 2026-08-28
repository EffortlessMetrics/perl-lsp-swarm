#!/usr/bin/env perl
#
# minimal_ptkdb_peer.pl -- two deliberately narrow Perl Debugger Peer
# Protocol adapters for perl-dap (perl-debug-peer-v1):
#
#   * executed directly: a synthetic reference peer used to inspect and test
#     the wire contract without Devel::ptkdb;
#   * loaded from .ptkdbrc: an experimental mirror plugin substrate for the
#     pinned Devel::ptkdb 1.1091 surface.
#
# The loaded plugin is not a DAP implementation and does not accept editor
# control. It authenticates to perl-dap, reports debugger-console output, emits
# real stopped events from Devel::ptkdb's set_file($path, $line) stop seam, and
# emits one terminated event during Perl teardown. Capabilities remain empty.
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
use Fcntl qw(F_GETFL F_SETFL O_NONBLOCK);
use IO::Select;
use IO::Socket::INET;
use JSON::PP qw(decode_json encode_json);
use Time::HiRes qw(time);

use constant PROTOCOL_VERSION     => 'perl-debug-peer-v1';
use constant PINNED_PTKDB_VERSION => '1.1091';
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
    my $flags = fcntl($socket, F_GETFL, 0);
    return 0 unless defined $flags;
    return defined fcntl($socket, F_SETFL, $flags | O_NONBLOCK);
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
    return (undef, "invalid PERL_DAP_PEER port '$port'")
        unless $port > 0 && $port <= 65_535;

    if ($require_token) {
        return (undef, 'PERL_DAP_PEER_TOKEN is required for the live ptkdb plugin')
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
    my $nonblocking = _set_nonblocking($socket) ? 1 : 0;

    my $state = {
        socket      => $socket,
        seq         => 0,
        read_buffer => '',
        closed      => 0,
        nonblocking => $nonblocking,
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
        unless ($response) {
            close($socket);
            return (undef, "peer/hello response failed: $read_error");
        }
        next unless ($response->{type} // '') eq 'response';
        next unless ($response->{requestSeq} // -1) == $hello_seq;
        next unless ($response->{command} // '') eq 'peer/hello';
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
    unless (defined $version && "$version" eq PINNED_PTKDB_VERSION) {
        _diagnostic(
            'live plugin requires Devel::ptkdb '
                . PINNED_PTKDB_VERSION
                . '; observed '
                . (defined $version ? $version : 'no loaded ptkdb version')
                . ' -- leaving ptkdb untouched'
        );
        return 1;
    }
    unless (defined &Devel::ptkdb::set_file) {
        _diagnostic('pinned Devel::ptkdb set_file seam is unavailable -- leaving ptkdb untouched');
        return 1;
    }

    my ($state, $error) = _connect_and_handshake(
        require_token => 1,
        peer          => 'Devel::ptkdb mirror plugin',
        peer_version  => PINNED_PTKDB_VERSION,
    );
    unless ($state) {
        _diagnostic("live plugin disabled: $error");
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
            . PINNED_PTKDB_VERSION
            . " mirror connected; reporting console/stopped/terminated only\n"
    );
    return 1;
}

sub run_reference_peer {
    my ($state, $error) = _connect_and_handshake(
        require_token => 0,
        peer          => 'minimal_ptkdb_peer.pl (reference)',
        peer_version  => '0.1',
    );
    die "minimal_ptkdb_peer.pl: $error\n" unless $state;

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
