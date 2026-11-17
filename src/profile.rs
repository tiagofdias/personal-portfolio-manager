//! Investor profiling: a short risk-tolerance / risk-capacity questionnaire.

use serde::{Deserialize, Serialize};

pub struct Question {
    pub id: &'static str,
    pub text: &'static str,
    /// (answer, score 0..=4). Higher = more risk can be taken.
    pub options: &'static [(&'static str, u8)],
}

pub const QUESTIONS: &[Question] = &[
    Question {
        id: "horizon",
        text: "When will you need most of this money?",
        options: &[
            ("In less than 2 years", 0),
            ("In 2-5 years", 1),
            ("In 5-10 years", 2),
            ("In 10-20 years", 3),
            ("In more than 20 years", 4),
        ],
    },
    Question {
        id: "drawdown",
        text: "Your portfolio falls 30% in a few months. What do you do?",
        options: &[
            ("Sell everything, I can't sleep", 0),
            ("Sell part of it to reduce risk", 1),
            ("Do nothing and wait", 2),
            ("Keep investing as planned", 3),
            ("Buy more, it's a discount", 4),
        ],
    },
    Question {
        id: "max_loss",
        text: "Largest one-year loss you could accept without changing plans?",
        options: &[("Up to 5%", 0), ("Up to 10%", 1), ("Up to 20%", 2), ("Up to 35%", 3), ("More than 35%", 4)],
    },
    Question {
        id: "goal",
        text: "What is the main goal of this portfolio?",
        options: &[
            ("Keep my capital safe", 0),
            ("Steady income", 1),
            ("Balanced growth and safety", 2),
            ("Long-term growth", 3),
            ("Maximum growth, I accept big swings", 4),
        ],
    },
    Question {
        id: "income",
        text: "How stable is your income?",
        options: &[
            ("No regular income / retired", 0),
            ("Unstable", 1),
            ("Fairly stable", 2),
            ("Stable", 3),
            ("Very stable with spare savings every month", 4),
        ],
    },
    Question {
        id: "emergency",
        text: "Do you have an emergency fund outside this portfolio?",
        options: &[("No", 0), ("Less than 3 months of expenses", 1), ("3-6 months", 3), ("More than 6 months", 4)],
    },
    Question {
        id: "experience",
        text: "How much investing experience do you have?",
        options: &[
            ("None", 0),
            ("Less than 2 years", 1),
            ("2-5 years, I've seen a downturn", 2),
            ("5+ years through several cycles", 3),
            ("Professional / very experienced", 4),
        ],
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    Conservative = 1,
    ModeratelyConservative = 2,
    Balanced = 3,
    Growth = 4,
    Aggressive = 5,
}

impl RiskLevel {
    pub fn from_level(n: u8) -> Self {
        match n {
            0 | 1 => Self::Conservative,
            2 => Self::ModeratelyConservative,
            3 => Self::Balanced,
            4 => Self::Growth,
            _ => Self::Aggressive,
        }
    }

    pub fn level(self) -> u8 {
        self as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Conservative => "Conservative",
            Self::ModeratelyConservative => "Moderately conservative",
            Self::Balanced => "Balanced",
            Self::Growth => "Growth",
            Self::Aggressive => "Aggressive",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Conservative => "Capital preservation first. Large drops would be hard to bear or would hurt your plans.",
            Self::ModeratelyConservative => "Some growth, but stability matters more than returns.",
            Self::Balanced => "Comfortable with normal market swings in exchange for reasonable long-term growth.",
            Self::Growth => "Long horizon and able to sit through deep drawdowns to pursue higher returns.",
            Self::Aggressive => "Maximum growth focus; can tolerate losses of 40-50%+ without abandoning the plan.",
        }
    }

    /// Reference strategic mix (equity, bonds, cash/gold) for this profile.
    pub fn target_mix(self) -> (f32, f32, f32) {
        match self {
            Self::Conservative => (25.0, 60.0, 15.0),
            Self::ModeratelyConservative => (45.0, 45.0, 10.0),
            Self::Balanced => (65.0, 30.0, 5.0),
            Self::Growth => (85.0, 10.0, 5.0),
            Self::Aggressive => (100.0, 0.0, 0.0),
        }
    }

    /// Maximum share for any single company (look-through) we consider prudent.
    pub fn max_single_stock(self) -> f32 {
        match self {
            Self::Conservative => 5.0,
            Self::ModeratelyConservative => 7.5,
            Self::Balanced => 10.0,
            Self::Growth => 15.0,
            Self::Aggressive => 20.0,
        }
    }

    /// Maximum share for "satellite" thematic/speculative positions.
    pub fn max_satellite(self) -> f32 {
        match self {
            Self::Conservative => 5.0,
            Self::ModeratelyConservative => 10.0,
            Self::Balanced => 15.0,
            Self::Growth => 25.0,
            Self::Aggressive => 35.0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Selected option index per question (same order as QUESTIONS).
    pub answers: Vec<Option<usize>>,
    /// Free notes learnt during the conversation (shown to the AI).
    #[serde(default)]
    pub notes: String,
}

impl Profile {
    pub fn ensure_len(&mut self) {
        self.answers.resize(QUESTIONS.len(), None);
    }

    pub fn answered(&self) -> usize {
        self.answers.iter().filter(|a| a.is_some()).count()
    }

    pub fn is_complete(&self) -> bool {
        self.answered() == QUESTIONS.len()
    }

    /// Average score 0..=4 over answered questions.
    fn avg(&self) -> Option<f32> {
        let scores: Vec<u8> = self
            .answers
            .iter()
            .zip(QUESTIONS)
            .filter_map(|(a, q)| a.and_then(|i| q.options.get(i)).map(|o| o.1))
            .collect();
        (!scores.is_empty()).then(|| scores.iter().map(|&s| s as f32).sum::<f32>() / scores.len() as f32)
    }

    fn score_of(&self, id: &str) -> Option<u8> {
        let i = QUESTIONS.iter().position(|q| q.id == id)?;
        let a = (*self.answers.get(i)?)?;
        Some(QUESTIONS[i].options[a].1)
    }

    /// Risk level. Capacity limits (short horizon, no emergency fund) cap the
    /// result even if the person *feels* comfortable with risk.
    pub fn risk_level(&self) -> Option<RiskLevel> {
        let avg = self.avg()?;
        let mut lvl = (avg + 1.0).round().clamp(1.0, 5.0) as u8;
        if let Some(h) = self.score_of("horizon") {
            lvl = lvl.min(h + 1);
        }
        if self.score_of("emergency") == Some(0) {
            lvl = lvl.min(3);
        }
        if let Some(d) = self.score_of("drawdown") {
            // Behaviour under stress weighs heavily: panic sellers can't be aggressive.
            lvl = lvl.min(d + 2);
        }
        Some(RiskLevel::from_level(lvl))
    }

    /// True when horizon, emergency fund or crash behaviour lowered the level
    /// below what the person's stated comfort with risk would give.
    pub fn is_capped(&self) -> bool {
        let Some(avg) = self.avg() else { return false };
        let raw = (avg + 1.0).round().clamp(1.0, 5.0) as u8;
        self.risk_level().is_some_and(|l| l.level() < raw)
    }

    pub fn summary(&self) -> String {
        let mut s = String::new();
        for (q, a) in QUESTIONS.iter().zip(&self.answers) {
            if let Some(i) = a {
                s.push_str(&format!("- {} -> {}\n", q.text, q.options[*i].0));
            }
        }
        if !self.notes.trim().is_empty() {
            s.push_str(&format!("- Notes: {}\n", self.notes.trim()));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_horizon_caps_risk() {
        let mut p = Profile::default();
        p.ensure_len();
        for (i, q) in QUESTIONS.iter().enumerate() {
            p.answers[i] = Some(q.options.len() - 1);
        }
        assert_eq!(p.risk_level(), Some(RiskLevel::Aggressive));
        p.answers[0] = Some(0); // money needed within 2 years
        assert_eq!(p.risk_level(), Some(RiskLevel::Conservative));
    }

    #[test]
    fn panic_seller_is_not_aggressive() {
        let mut p = Profile::default();
        p.ensure_len();
        for (i, q) in QUESTIONS.iter().enumerate() {
            p.answers[i] = Some(q.options.len() - 1);
        }
        p.answers[1] = Some(0);
        assert!(p.risk_level().unwrap() <= RiskLevel::ModeratelyConservative);
    }
}
