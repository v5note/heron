//! Token budget of enabling a set of skills (design risk R6).

use std::collections::BTreeMap;

/// Advisory threshold above which the UI warns before bulk enabling.
pub const WARN_THRESHOLD: u32 = 5_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BudgetEstimate {
    pub by_target: BTreeMap<String, u32>,
    pub total: u32,
}

impl BudgetEstimate {
    pub fn warns(&self) -> bool {
        self.total > WARN_THRESHOLD
    }

    pub fn add(&mut self, target: &str, tokens: u32) {
        *self.by_target.entry(target.to_owned()).or_default() += tokens;
        self.total += tokens;
    }
}

/// Rough catalog cost of one skill. Deliberately pessimistic (~3.5 chars per token).
pub fn estimate_tokens(name: &str, description: &str) -> u32 {
    let chars = (name.chars().count() + description.chars().count()) as f64;
    (chars / 3.5).ceil() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_and_warns() {
        assert_eq!(estimate_tokens("pdf", "Extract text"), 5);
        let mut b = BudgetEstimate::default();
        b.add("claude:global", 4_000);
        b.add("heron:global", 2_000);
        assert_eq!(b.total, 6_000);
        assert!(b.warns());
        assert_eq!(b.by_target["claude:global"], 4_000);
    }
}
