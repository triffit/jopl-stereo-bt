/// What a render cycle should do with its input queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayoutPlan {
    /// Frames to discard from the front of the queue before reading.
    pub skip: usize,
    /// Frames to read after skipping; the rest of the request is silence.
    pub read: usize,
}

/// Decides how many queued frames a device plays per render cycle.
///
/// Keeps every output's queue near the same `target` fill level so all
/// devices have the same base latency, independent of capture timing:
/// - Plays silence until `target` frames are queued (priming), then
///   discards any surplus so playback starts at exactly `target` latency.
/// - On underrun, plays what is left and primes again.
/// - When clock drift lets the queue grow beyond `target + max_excess`,
///   trims it back to `target`.
///
/// Pure bookkeeping; allocation-free and safe on a real-time thread.
#[derive(Debug, Clone)]
pub struct Playout {
    target: usize,
    max_excess: usize,
    primed: bool,
    underruns: u64,
    trims: u64,
}

impl Playout {
    /// Creates an unprimed playout with the given cushion and drift limit, in frames.
    #[must_use]
    pub const fn new(target: usize, max_excess: usize) -> Self {
        Self {
            target,
            max_excess,
            primed: false,
            underruns: 0,
            trims: 0,
        }
    }

    /// Plans one render cycle given `available` queued frames and a device
    /// request for `requested` frames.
    pub fn plan(&mut self, available: usize, requested: usize) -> PlayoutPlan {
        let skip = if self.primed {
            if available > self.target + self.max_excess {
                self.trims += 1;
                available - self.target
            } else {
                0
            }
        } else {
            if available < self.target {
                return PlayoutPlan { skip: 0, read: 0 };
            }
            // Start every output at exactly the same queue latency.
            self.primed = true;
            available - self.target
        };
        let remaining = available - skip;

        let read = remaining.min(requested);
        if read < requested {
            self.primed = false;
            self.underruns += 1;
        }
        PlayoutPlan { skip, read }
    }

    /// Forgets the primed state, e.g. after the stream restarts.
    pub fn reset(&mut self) {
        self.primed = false;
    }

    /// Whether the cushion has been reached and audio is flowing.
    #[must_use]
    pub const fn is_primed(&self) -> bool {
        self.primed
    }

    /// Number of underruns since creation.
    #[must_use]
    pub const fn underruns(&self) -> u64 {
        self.underruns
    }

    /// Number of drift trims since creation.
    #[must_use]
    pub const fn trims(&self) -> u64 {
        self.trims
    }
}
