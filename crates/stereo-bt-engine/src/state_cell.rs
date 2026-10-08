use std::sync::atomic::{AtomicU8, Ordering};

use crate::StreamState;

/// A [`StreamState`] shared lock-free between a stream thread and the control side.
#[derive(Debug)]
pub(crate) struct StateCell(AtomicU8);

impl Default for StateCell {
    fn default() -> Self {
        Self::new()
    }
}

impl StateCell {
    pub(crate) const fn new() -> Self {
        Self(AtomicU8::new(to_u8(StreamState::Starting)))
    }

    pub(crate) fn get(&self) -> StreamState {
        from_u8(self.0.load(Ordering::Relaxed))
    }

    pub(crate) fn set(&self, state: StreamState) {
        self.0.store(to_u8(state), Ordering::Relaxed);
    }
}

const fn to_u8(state: StreamState) -> u8 {
    match state {
        StreamState::Starting => 0,
        StreamState::Running => 1,
        StreamState::Failed => 2,
        StreamState::Stopped => 3,
    }
}

const fn from_u8(value: u8) -> StreamState {
    match value {
        1 => StreamState::Running,
        2 => StreamState::Failed,
        3 => StreamState::Stopped,
        _ => StreamState::Starting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_in_starting_state() {
        assert_eq!(StateCell::new().get(), StreamState::Starting);
    }

    #[test]
    fn every_stream_state_round_trips() {
        let cell = StateCell::new();
        for state in [
            StreamState::Starting,
            StreamState::Running,
            StreamState::Failed,
            StreamState::Stopped,
        ] {
            cell.set(state);
            assert_eq!(cell.get(), state);
        }
    }
}
