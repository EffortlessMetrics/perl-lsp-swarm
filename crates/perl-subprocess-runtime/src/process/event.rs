//! The ordered event stream a run emits, and the ledger that orders it.

use super::identity::RunId;
use super::result::{CancellationReason, StreamChannel, TerminalDisposition};

/// A run-scoped monotonic sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventSequence(u64);

impl EventSequence {
    /// The sequence number's value.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// A bounded description of bytes seen on one channel.
///
/// The payload is a length and an offset, not the bytes: events are a control
/// stream, and the content identity lives in the result's stream evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// The channel is *not* a field here: [`ProcessEventKind::StdoutBytes`] and
/// [`ProcessEventKind::StderrBytes`] already name it, and a second copy could
/// disagree with the variant carrying it — letting a consumer attribute output
/// to the wrong stream despite valid sequencing.
pub struct StreamChunkEvidence {
    /// How many bytes arrived.
    pub byte_count: u64,
    /// How many bytes had already been observed on this channel.
    pub offset: u64,
    /// Whether these bytes were retained or only counted.
    pub retained: bool,
}

/// Which budget was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitEvidence {
    /// The channel whose budget was reached.
    pub channel: StreamChannel,
    /// The limit that was reached.
    pub limit_bytes: u64,
    /// Whether the run continues after the limit.
    pub run_continues: bool,
}

/// A step in terminating a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationPhase {
    /// A cancellation was requested. Requesting is not cleaning up.
    CancellationRequested(CancellationReason),
    /// The deadline elapsed.
    DeadlineReached,
    /// A graceful termination signal was sent.
    GracefulSignalSent,
    /// A forced termination signal was sent.
    ForcedSignalSent,
    /// The owned process group was reaped.
    GroupReaped,
}

/// What a run emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEventKind {
    /// The child was created.
    Started,
    /// Bytes arrived on stdout.
    StdoutBytes(StreamChunkEvidence),
    /// Bytes arrived on stderr.
    StderrBytes(StreamChunkEvidence),
    /// A capture budget was reached.
    LimitReached(LimitEvidence),
    /// A termination step was taken.
    TerminationPhase(TerminationPhase),
    /// The run settled. Nothing may follow.
    Terminal(TerminalDisposition),
}

impl ProcessEventKind {
    /// Whether this event settles the run.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal(_))
    }
}

/// One ordered event from one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessEvent {
    run_id: RunId,
    sequence: EventSequence,
    kind: ProcessEventKind,
}

impl ProcessEvent {
    /// The run that emitted the event.
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    /// The event's position in the run's stream.
    pub fn sequence(&self) -> EventSequence {
        self.sequence
    }

    /// What happened.
    pub fn kind(&self) -> &ProcessEventKind {
        &self.kind
    }
}

/// Why an event could not be admitted to a run's stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventAdmissionError {
    /// The run already settled; no event may follow terminal settlement.
    AfterTerminalSettlement,
    /// The run's sequence space is exhausted.
    SequenceExhausted,
    /// A chunk's offset does not continue from what the channel already saw.
    ///
    /// An offset that skips ahead hides bytes; one that goes back double-counts
    /// them. Either way the event stream no longer reassembles into what the
    /// run produced.
    ChunkOffsetDiscontinuous {
        /// The offset the channel's admitted bytes require.
        expected: u64,
        /// The offset the chunk claimed.
        found: u64,
    },
}

impl std::fmt::Display for EventAdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AfterTerminalSettlement => f.write_str("no event may follow terminal settlement"),
            Self::SequenceExhausted => f.write_str("run event sequence space is exhausted"),
            Self::ChunkOffsetDiscontinuous { expected, found } => {
                write!(f, "chunk offset {found} does not continue from {expected}")
            }
        }
    }
}

impl std::error::Error for EventAdmissionError {}

/// Assigns sequence numbers and enforces terminal settlement for one run.
///
/// A supervisor backend owns one ledger per run. The ledger is the reason
/// "no event follows terminal settlement" is a property of the domain rather
/// than a convention each backend has to remember.
#[derive(Debug)]
pub struct EventLedger {
    run_id: RunId,
    next_sequence: u64,
    settled: bool,
    stdout_observed: u64,
    stderr_observed: u64,
}

impl EventLedger {
    /// Open a ledger for a run.
    pub fn new(run_id: RunId) -> Self {
        Self { run_id, next_sequence: 0, settled: false, stdout_observed: 0, stderr_observed: 0 }
    }

    /// The run this ledger orders.
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    /// How many bytes this ledger has admitted for a channel.
    ///
    /// Exposed so a backend can reconcile the event stream against the
    /// `observed_bytes` it puts in the result. The ledger cannot do that join
    /// itself — it never sees the result — so whoever holds both halves must.
    pub fn observed_bytes(&self, channel: StreamChannel) -> u64 {
        match channel {
            StreamChannel::Stdout => self.stdout_observed,
            StreamChannel::Stderr => self.stderr_observed,
        }
    }

    /// Whether the run has settled.
    pub fn is_settled(&self) -> bool {
        self.settled
    }

    /// How many events have been admitted.
    pub fn admitted_count(&self) -> u64 {
        self.next_sequence
    }

    /// Admit an event, assigning it the next sequence number.
    pub fn admit(&mut self, kind: ProcessEventKind) -> Result<ProcessEvent, EventAdmissionError> {
        if self.settled {
            return Err(EventAdmissionError::AfterTerminalSettlement);
        }
        let Some(next) = self.next_sequence.checked_add(1) else {
            return Err(EventAdmissionError::SequenceExhausted);
        };
        // A chunk's offset is how much of its channel was already seen, so it
        // must be exactly what this ledger has admitted for that channel. An
        // offset that skips forward hides bytes; one that goes backward
        // double-counts them. Either way a consumer reassembling the stream
        // from these events gets something the run never produced, so the
        // offset is verified rather than trusted.
        let chunk = match &kind {
            ProcessEventKind::StdoutBytes(evidence) => Some((evidence, self.stdout_observed)),
            ProcessEventKind::StderrBytes(evidence) => Some((evidence, self.stderr_observed)),
            _ => None,
        };
        if let Some((evidence, already_observed)) = chunk {
            if evidence.offset != already_observed {
                return Err(EventAdmissionError::ChunkOffsetDiscontinuous {
                    expected: already_observed,
                    found: evidence.offset,
                });
            }
            let Some(total) = already_observed.checked_add(evidence.byte_count) else {
                return Err(EventAdmissionError::SequenceExhausted);
            };
            match kind {
                ProcessEventKind::StdoutBytes(_) => self.stdout_observed = total,
                _ => self.stderr_observed = total,
            }
        }
        let sequence = EventSequence(self.next_sequence);
        self.next_sequence = next;
        if kind.is_terminal() {
            self.settled = true;
        }
        Ok(ProcessEvent { run_id: self.run_id.clone(), sequence, kind })
    }
}
