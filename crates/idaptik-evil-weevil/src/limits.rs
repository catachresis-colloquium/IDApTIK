//! Hard, explicit bounds for every adversarial run.
//!
//! Evil Weevil is an authorised, *bounded* test instrument, not a load
//! generator or an availability tool. Every scenario's input sequence is a
//! fixed, in-memory list checked against these constants before it touches a
//! production validator, and the harness fails the scenario the moment a bound
//! is crossed — it never waits to see what happens next. There is no wall
//! clock in the sans-IO core: "timeouts" are logical step budgets, so the same
//! bounds hold on every machine and every run is reproducible.

/// Maximum number of lockstep steps one scenario may execute. A run that
/// reaches this without converging is a harness defect, not a test result.
pub const MAX_STEPS: u64 = 500;

/// Maximum number of wire messages (commands, commits and control frames
/// counted together) one scenario may put through the in-memory wire.
pub const MAX_WIRE_MESSAGES: usize = 512;

/// Maximum depth of any per-seat pending queue. This is the queue-growth
/// measurement: the harness records the observed peak and fails if it ever
/// reaches this cap, so an unbounded-queue regression is a red test.
pub const MAX_QUEUE_DEPTH: usize = 64;

/// Maximum number of intents a single adversarial agent may propose in one
/// scenario. The corpus's fixed sequences are all far below this; it exists to
/// make an accidental loop a failure instead of a hang.
pub const MAX_INTENTS_PER_SCENARIO: usize = 32;

/// A measured resource profile, attached to every scenario result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HarnessMetrics {
    /// Lockstep steps actually executed.
    pub steps: u64,
    /// Total messages placed on the in-memory wire.
    pub wire_messages: usize,
    /// The largest pending-queue depth observed on either seat.
    pub peak_queue_depth: usize,
    /// Intents the adversarial agent proposed.
    pub intents: usize,
}

impl Default for HarnessMetrics {
    fn default() -> Self {
        Self {
            steps: 0,
            wire_messages: 0,
            peak_queue_depth: 0,
            intents: 0,
        }
    }
}

impl HarnessMetrics {
    /// Whether every measurement stayed inside the strict test limits.
    pub fn within_limits(&self) -> bool {
        self.steps <= MAX_STEPS
            && self.wire_messages <= MAX_WIRE_MESSAGES
            && self.peak_queue_depth < MAX_QUEUE_DEPTH
            && self.intents <= MAX_INTENTS_PER_SCENARIO
    }

    /// Human-readable names of the limits that were crossed, if any.
    pub fn violations(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.steps > MAX_STEPS {
            out.push("MAX_STEPS");
        }
        if self.wire_messages > MAX_WIRE_MESSAGES {
            out.push("MAX_WIRE_MESSAGES");
        }
        if self.peak_queue_depth >= MAX_QUEUE_DEPTH {
            out.push("MAX_QUEUE_DEPTH");
        }
        if self.intents > MAX_INTENTS_PER_SCENARIO {
            out.push("MAX_INTENTS_PER_SCENARIO");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_profile_is_within_limits() {
        assert!(HarnessMetrics::default().within_limits());
        assert!(HarnessMetrics {
            steps: MAX_STEPS,
            wire_messages: MAX_WIRE_MESSAGES,
            peak_queue_depth: MAX_QUEUE_DEPTH - 1,
            intents: MAX_INTENTS_PER_SCENARIO,
        }
        .within_limits());
    }

    #[test]
    fn every_crossed_limit_is_named() {
        let m = HarnessMetrics {
            steps: MAX_STEPS + 1,
            wire_messages: MAX_WIRE_MESSAGES + 1,
            peak_queue_depth: MAX_QUEUE_DEPTH,
            intents: MAX_INTENTS_PER_SCENARIO + 1,
        };
        assert!(!m.within_limits());
        assert_eq!(
            m.violations(),
            vec![
                "MAX_STEPS",
                "MAX_WIRE_MESSAGES",
                "MAX_QUEUE_DEPTH",
                "MAX_INTENTS_PER_SCENARIO",
            ]
        );
    }
}
