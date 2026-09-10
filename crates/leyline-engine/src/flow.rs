//! Whether a batch keeps going — the answer a progress callback gives back
//! (ADR 0139).
//!
//! Every long operation in this crate already reports `(done, total)` after
//! each unit of work. That call is the only place where a batch is provably
//! between two photographs, with nothing half-written, so it is also the
//! place where it can be asked to stop: the callback answers, and the loop
//! obeys.
//!
//! A caller that has no cancelling to do writes `|_, _| {}` as before — `()`
//! converts to [`Flow::Continue`], so the hundred existing call sites keep
//! their shape and a batch nobody can stop simply never stops.

/// What a progress callback answers: keep going, or stop here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Flow {
    /// Carry on with the next unit.
    #[default]
    Continue,
    /// Stop the batch. What is already written stays written, and the
    /// report says the run was cut short.
    Stop,
}

impl Flow {
    /// Whether the batch must stop now.
    #[must_use]
    pub fn stops(self) -> bool {
        self == Self::Stop
    }
}

impl From<()> for Flow {
    /// A callback that answers nothing has nothing to stop.
    fn from((): ()) -> Self {
        Self::Continue
    }
}

impl From<bool> for Flow {
    /// `true` keeps going — the reading of "carry on?" rather than "stop?",
    /// chosen because every caller that returns a boolean here is answering
    /// the first question.
    fn from(carry_on: bool) -> Self {
        if carry_on { Self::Continue } else { Self::Stop }
    }
}

#[cfg(test)]
mod tests {
    use super::Flow;

    #[test]
    fn a_callback_that_answers_nothing_carries_on() {
        assert_eq!(Flow::from(()), Flow::Continue);
        assert!(!Flow::from(()).stops());
    }

    #[test]
    fn a_boolean_reads_as_carry_on() {
        assert_eq!(Flow::from(true), Flow::Continue);
        assert_eq!(Flow::from(false), Flow::Stop);
        assert!(Flow::from(false).stops());
    }
}
