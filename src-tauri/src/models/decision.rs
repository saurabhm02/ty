use crate::config::{SURE_NO_PERCENTAGE, SURE_YES_PERCENTAGE};

/// How a yes/no question was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Yes,
    No,
    /// The model was not sure either way, so nobody should act on it.
    Unsure,
}

/// A verdict together with the number it came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decision {
    pub verdict: Verdict,
    /// How likely "yes" is, from 0.0 (never) to 1.0 (certain).
    pub chances_yes: f32,
}

/// Turns the chance of "yes" into a verdict.
pub fn decide_verdict(chances_yes: f32) -> Verdict {
    if chances_yes >= SURE_YES_PERCENTAGE {
        Verdict::Yes
    } else if chances_yes <= SURE_NO_PERCENTAGE {
        Verdict::No
    } else {
        Verdict::Unsure
    }
}

impl Decision {
    pub fn from_chance(chances_yes: f32) -> Self {
        Self {
            verdict: decide_verdict(chances_yes),
            chances_yes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_high_chance_is_a_yes() {
        assert_eq!(decide_verdict(0.97), Verdict::Yes);
    }

    #[test]
    fn a_low_chance_is_a_no() {
        assert_eq!(decide_verdict(0.02), Verdict::No);
    }

    #[test]
    fn a_middle_chance_is_unsure() {
        assert_eq!(decide_verdict(0.5), Verdict::Unsure);
    }

    #[test]
    fn the_edges_count_as_sure() {
        assert_eq!(decide_verdict(SURE_YES_PERCENTAGE), Verdict::Yes);
        assert_eq!(decide_verdict(SURE_NO_PERCENTAGE), Verdict::No);
    }

    #[test]
    fn a_decision_keeps_the_number_it_came_from() {
        let decision = Decision::from_chance(0.9);
        assert_eq!(decision.verdict, Verdict::Yes);
        assert_eq!(decision.chances_yes, 0.9);
    }
}
