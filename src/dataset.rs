//! Synthetic fine-tuning data. Each example pairs a random portfolio and
//! investor profile with an answer grounded in the deterministic engine, in
//! the same prompt format the app uses at inference time.
//!
//! The templated answers teach structure, grounding and the <portfolio>
//! block. For the best model, have people review/rewrite a subset (see
//! ai/finetune/README.md).

use crate::ai::{self, build_system};
use crate::analysis::{self, Report, Severity};
use crate::assets::AssetDb;
use crate::portfolio::{Holding, Portfolio};
use crate::profile::{Profile, QUESTIONS};
use std::io::Write;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const USER_PROMPTS: &[&str] = &[
    "Analyse my portfolio. What is good, what is bad and how can I improve it?",
    "Is my portfolio OK for someone like me?",
    "What kind of investor does this portfolio suit, and what should I change?",
    "Analisa o meu portfólio: o que está bem, o que está mal e como melhorar?",
    "Give me an honest review of my allocation.",
];

fn random_portfolio(db: &AssetDb, rng: &mut Rng) -> Portfolio {
    let assets = db.all();
    let n = 1 + rng.below(5);
    let mut p = Portfolio::default();
    let mut weights: Vec<f32> = (0..n).map(|_| 1.0 + rng.below(20) as f32).collect();
    let sum: f32 = weights.iter().sum();
    // 1 in 6 portfolios is deliberately incomplete (< 100%).
    let target = if rng.below(6) == 0 { 50.0 + rng.below(45) as f32 } else { 100.0 };
    for w in &mut weights {
        *w = (*w * target / sum / 5.0).round() * 5.0;
    }
    for w in weights {
        let a = &assets[rng.below(assets.len())];
        if w > 0.0 {
            p.add(Holding { ticker: a.ticker.clone(), percent: w, known: true });
        }
    }
    p.sort();
    p
}

fn random_profile(rng: &mut Rng) -> Profile {
    let mut p = Profile::default();
    p.ensure_len();
    // Correlated answers look more like real people than independent noise.
    let base = rng.below(5) as i32;
    for (i, q) in QUESTIONS.iter().enumerate() {
        if rng.below(10) == 0 {
            continue; // sometimes unanswered
        }
        let jitter = rng.below(3) as i32 - 1;
        let idx = (base + jitter).clamp(0, q.options.len() as i32 - 1) as usize;
        p.answers[i] = Some(idx);
    }
    p
}

fn answer(r: &Report, p: &Portfolio, prof: &Profile) -> String {
    let mut s = String::new();
    s.push_str(&format!("Overall health: {}/100.\n\n", r.health));
    let goods: Vec<_> = r.findings.iter().filter(|f| f.severity == Severity::Good).collect();
    let bads: Vec<_> = r.findings.iter().filter(|f| f.severity >= Severity::Warning).collect();
    if !bads.is_empty() {
        s.push_str("Main risks:\n");
        for f in bads {
            s.push_str(&format!("- {}. {}\n", f.title, f.detail.replace('\n', " ")));
        }
        s.push('\n');
    }
    if !goods.is_empty() {
        s.push_str("What is good:\n");
        for f in goods {
            s.push_str(&format!("- {}\n", f.title));
        }
        s.push('\n');
    }
    match (prof.risk_level(), r.revealed) {
        (Some(pl), Some(rv)) => s.push_str(&format!(
            "Investor type: your answers say {}, while this portfolio behaves like a {} portfolio.\n\n",
            pl.label().to_lowercase(),
            rv.label().to_lowercase()
        )),
        (None, _) => s.push_str("To tailor this, tell me: when will you need this money, and what would you do if it fell 30%?\n\n"),
        _ => {}
    }
    if !r.suggestions.is_empty() {
        s.push_str("How to improve:\n");
        for x in &r.suggestions {
            s.push_str(&format!("- {x}\n"));
        }
        s.push('\n');
    }
    if let Some(pp) = &r.proposal {
        if pp != p {
            s.push_str("A version that fits your profile:\n<portfolio>\n");
            for h in &pp.holdings {
                s.push_str(&format!("{} {}\n", h.ticker, h.percent));
            }
            s.push_str("</portfolio>\n\n");
        }
    }
    s.push_str("This is educational information, not personal financial advice.");
    s
}

pub fn generate(n: usize, path: &str) -> std::io::Result<()> {
    let db = AssetDb::load(None);
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1;
    let mut rng = Rng(seed);
    if let Some(dir) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    for _ in 0..n {
        let p = random_portfolio(&db, &mut rng);
        let prof = random_profile(&mut rng);
        let r = analysis::analyse(&p, &db, prof.risk_level());
        let system = build_system(&prof, &analysis::to_text(&r, &p));
        let user = format!("{} My portfolio: {}", USER_PROMPTS[rng.below(USER_PROMPTS.len())], p.describe());
        let line = serde_json::json!({ "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
            { "role": "assistant", "content": answer(&r, &p, &prof) },
        ]});
        writeln!(out, "{line}")?;
    }
    let _ = ai::SYSTEM_PROMPT;
    out.flush()
}
