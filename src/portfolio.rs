//! Portfolio model and a small natural-language parser for allocations
//! written as percentages, e.g. "I have 25% NVIDIA and 50% VVSM ETF".

use crate::assets::{AssetDb, normalize};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Holding {
    /// Resolved ticker, or the user's own words when the asset is unknown.
    pub ticker: String,
    pub percent: f32,
    pub known: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Portfolio {
    pub holdings: Vec<Holding>,
}

impl Portfolio {
    pub fn total(&self) -> f32 {
        // `+ 0.0` turns the empty-sum identity (-0.0) into 0.0.
        self.holdings.iter().map(|h| h.percent).sum::<f32>() + 0.0
    }

    pub fn is_empty(&self) -> bool {
        self.holdings.is_empty()
    }

    /// Adds to an existing line when the same asset appears twice.
    pub fn add(&mut self, h: Holding) {
        if let Some(e) = self.holdings.iter_mut().find(|e| e.ticker == h.ticker) {
            e.percent += h.percent;
        } else {
            self.holdings.push(h);
        }
    }

    pub fn fill_remainder_with_cash(&mut self) {
        let rest = 100.0 - self.total();
        if rest > 0.05 {
            self.add(Holding { ticker: "CASH".into(), percent: rest, known: true });
        }
    }

    /// Rescales every line so the total is exactly 100 %.
    pub fn normalize(&mut self) {
        let t = self.total();
        if t > 0.0 {
            for h in &mut self.holdings {
                h.percent = h.percent * 100.0 / t;
            }
        }
    }

    pub fn sort(&mut self) {
        self.holdings.sort_by(|a, b| b.percent.total_cmp(&a.percent));
    }

    /// Compact one-line description used in prompts and the UI.
    pub fn describe(&self) -> String {
        self.holdings
            .iter()
            .map(|h| format!("{} {:.1}%", h.ticker, h.percent))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug, PartialEq)]
enum Tok {
    Pct(f32),
    Word(String),
    Sep,
}

/// Words that never belong to an asset name.
const FILLER: &[&str] = &[
    "i", "im", "have", "has", "got", "own", "hold", "holding", "my", "portfolio", "is", "are", "in",
    "of", "on", "the", "a", "an", "about", "around", "roughly", "approx", "approximately", "with",
    "into", "invested", "allocation", "percent", "pct", "etf", "etfs", "stock", "stocks", "shares",
    "share", "fund", "index", "ucits", "acc", "dist", "position", "also", "plus", "rest",
    // Portuguese / Spanish basics
    "tenho", "eu", "em", "de", "do", "da", "no", "na", "meu", "minha", "acoes", "acao", "tengo",
    "mi", "en", "del", "y",
];

const SEPARATOR_WORDS: &[&str] = &["and", "e", "y", "plus", "then"];

fn tokenize(input: &str) -> Vec<Tok> {
    let mut toks = Vec::new();
    // Treat punctuation separators as their own tokens, except a decimal
    // comma between digits ("12,5%").
    let chars: Vec<char> = input.chars().collect();
    let mut spaced = String::with_capacity(input.len() + 8);
    for (i, &c) in chars.iter().enumerate() {
        let decimal_comma = c == ','
            && i > 0
            && chars[i - 1].is_ascii_digit()
            && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit());
        match c {
            ',' if decimal_comma => spaced.push(','),
            ',' | ';' | '\n' | '+' | '|' | '/' => spaced.push_str(" ; "),
            '%' => spaced.push_str("% "),
            _ => spaced.push(c),
        }
    }

    let words: Vec<&str> = spaced.split_whitespace().collect();
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        if w == ";" {
            toks.push(Tok::Sep);
            i += 1;
            continue;
        }
        let lw = w.to_lowercase();
        let trimmed = lw.trim_matches(|c: char| matches!(c, ':' | '=' | '(' | ')' | '.' | '!' | '?'));
        if SEPARATOR_WORDS.contains(&trimmed) {
            toks.push(Tok::Sep);
            i += 1;
            continue;
        }
        // Number, optionally glued to '%' ("25%", "12,5%"), or followed by "%"/"percent".
        let num_part = trimmed.trim_end_matches('%').replace(',', ".");
        if let Ok(v) = num_part.parse::<f32>() {
            let glued = trimmed.ends_with('%');
            let next = words.get(i + 1).map(|s| s.to_lowercase());
            let next_is_pct = matches!(next.as_deref(), Some("%" | "percent" | "pct" | "por" | "porcento" | "%."));
            if glued || next_is_pct {
                toks.push(Tok::Pct(v));
                i += if next_is_pct && !glued { 2 } else { 1 };
                continue;
            }
            // Bare number like "500" in "S&P 500" is part of a name.
        }
        if trimmed == "%" {
            i += 1;
            continue;
        }
        if !trimmed.is_empty() {
            toks.push(Tok::Word(w.trim_matches(|c: char| matches!(c, ':' | '=' | '(' | ')' | '!' | '?')).to_string()));
        }
        i += 1;
    }
    toks
}

/// Raw (name, percent) pairs found in free text.
fn extract_pairs(input: &str) -> Vec<(String, f32)> {
    #[derive(Default)]
    struct Group {
        name: Vec<String>,
        pct: Option<f32>,
        pct_first: bool,
    }
    let mut out = Vec::new();
    let mut g = Group::default();
    let flush = |g: &mut Group, out: &mut Vec<(String, f32)>| {
        let name: Vec<&String> =
            g.name.iter().filter(|w| !FILLER.contains(&w.to_lowercase().as_str())).collect();
        if let (Some(p), false) = (g.pct, name.is_empty()) {
            out.push((name.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "), p));
        }
        *g = Group::default();
    };
    let has_name = |g: &Group| g.name.iter().any(|w| !FILLER.contains(&w.to_lowercase().as_str()));

    for t in tokenize(input) {
        match t {
            Tok::Sep => {
                // "50% Vanguard and Microsoft" -> keep joining only if no pct yet.
                if g.pct.is_some() {
                    flush(&mut g, &mut out);
                }
            }
            Tok::Pct(v) => {
                if g.pct.is_some() && has_name(&g) {
                    flush(&mut g, &mut out);
                }
                if g.pct.is_none() {
                    g.pct_first = !has_name(&g);
                }
                g.pct = Some(v);
            }
            Tok::Word(w) => {
                let is_filler = FILLER.contains(&w.to_lowercase().as_str());
                // "NVIDIA 25% VVSM 50%": a new name after a name-then-pct group.
                if !is_filler && g.pct.is_some() && !g.pct_first && has_name(&g) {
                    flush(&mut g, &mut out);
                }
                g.name.push(w);
            }
        }
    }
    flush(&mut g, &mut out);
    out
}

/// Resolve a free-text name to a known asset: the whole phrase first, then
/// the longest contiguous sub-phrase that matches.
fn resolve<'a>(db: &'a AssetDb, phrase: &str) -> Option<&'a crate::assets::Asset> {
    if let Some(a) = db.find(phrase) {
        return Some(a);
    }
    let words: Vec<&str> = phrase.split_whitespace().collect();
    for len in (1..words.len()).rev() {
        for start in 0..=words.len() - len {
            let sub = words[start..start + len].join(" ");
            if normalize(&sub).len() < 2 {
                continue;
            }
            if let Some(a) = db.find(&sub) {
                return Some(a);
            }
        }
    }
    None
}

pub struct ParseResult {
    pub portfolio: Portfolio,
    pub unknown: Vec<String>,
}

pub fn parse(db: &AssetDb, input: &str) -> ParseResult {
    let mut p = Portfolio::default();
    let mut unknown = Vec::new();
    for (name, pct) in extract_pairs(input) {
        if pct <= 0.0 {
            continue;
        }
        match resolve(db, &name) {
            Some(a) => p.add(Holding { ticker: a.ticker.clone(), percent: pct, known: true }),
            None => {
                let t = name.to_uppercase();
                unknown.push(t.clone());
                p.add(Holding { ticker: t, percent: pct, known: false });
            }
        }
    }
    p.sort();
    ParseResult { portfolio: p, unknown }
}

/// Parse a machine-readable proposal block: one "NAME NUMBER[%]" per line.
/// Unlike free text, a bare number here is a percentage.
pub fn parse_block(db: &AssetDb, block: &str) -> Portfolio {
    let mut p = Portfolio::default();
    for line in block.lines() {
        let line = line.trim().trim_start_matches(['-', '*', '•']).trim();
        let Some((name, num)) = line.rsplit_once(|c: char| c.is_whitespace() || c == ':') else { continue };
        let num = num.trim().trim_end_matches('%').replace(',', ".");
        let Ok(pct) = num.parse::<f32>() else { continue };
        let name = name.trim().trim_end_matches(':').trim();
        if name.is_empty() || pct <= 0.0 {
            continue;
        }
        match resolve(db, name) {
            Some(a) => p.add(Holding { ticker: a.ticker.clone(), percent: pct, known: true }),
            None => p.add(Holding { ticker: name.to_uppercase(), percent: pct, known: false }),
        }
    }
    p.sort();
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposal_block_with_bare_numbers() {
        let db = AssetDb::load(None);
        let p = parse_block(&db, "VWCE 60  \nVVSM 16.5\n- AGGH: 10%\nNVDA 8.5\nCASH 5\n");
        assert_eq!(p.describe(), "VWCE 60.0%, VVSM 16.5%, AGGH 10.0%, NVDA 8.5%, CASH 5.0%");
    }

    fn tickers(input: &str) -> Vec<(String, f32)> {
        let db = AssetDb::load(None);
        parse(&db, input).portfolio.holdings.into_iter().map(|h| (h.ticker, h.percent)).collect()
    }

    #[test]
    fn user_example() {
        assert_eq!(
            tickers("I have 25% NVIDIA and 50% VVSM ETF"),
            vec![("VVSM".into(), 50.0), ("NVDA".into(), 25.0)]
        );
    }

    #[test]
    fn name_before_percent_and_commas() {
        assert_eq!(
            tickers("NVIDIA 25%, S&P 500 60 %, gold 15%"),
            vec![("SPY".into(), 60.0), ("NVDA".into(), 25.0), ("GOLD".into(), 15.0)]
        );
    }

    #[test]
    fn no_separators_and_decimal_comma() {
        assert_eq!(
            tickers("12,5% bitcoin 87.5% vanguard ftse all world"),
            vec![("VWCE".into(), 87.5), ("BTC".into(), 12.5)]
        );
        assert_eq!(tickers("NVDA 30% MSFT 70%"), vec![("MSFT".into(), 70.0), ("NVDA".into(), 30.0)]);
    }

    #[test]
    fn unknown_assets_are_kept() {
        let db = AssetDb::load(None);
        let r = parse(&db, "40% Some Small Cap Co and 60 percent msci world");
        assert_eq!(r.unknown, vec!["SOME SMALL CAP CO".to_string()]);
        assert_eq!(r.portfolio.total(), 100.0);
    }

    #[test]
    fn duplicates_merge() {
        assert_eq!(tickers("10% nvda, 15% nvidia"), vec![("NVDA".into(), 25.0)]);
    }
}
