//! Reference database of known assets (embedded at compile time, optionally
//! extended by a user file). Lookups are O(1) through a normalised alias index.

use serde::Deserialize;
use std::collections::HashMap;

const EMBEDDED: &str = include_str!("../data/assets.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum AssetClass {
    Equity,
    Bonds,
    Cash,
    Commodity,
    Crypto,
    RealEstate,
}

impl AssetClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::Equity => "Equities",
            Self::Bonds => "Bonds",
            Self::Cash => "Cash",
            Self::Commodity => "Commodities",
            Self::Crypto => "Crypto",
            Self::RealEstate => "Real estate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Vehicle {
    Stock,
    ETF,
    Coin,
    Cash,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub ticker: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub vehicle: Vehicle,
    pub class: AssetClass,
    /// 1 (very low volatility) ..= 5 (very high).
    pub risk: u8,
    /// Total expense ratio in percent per year.
    #[serde(default)]
    pub ter: Option<f32>,
    #[serde(default)]
    pub sectors: Vec<(String, f32)>,
    #[serde(default)]
    pub regions: Vec<(String, f32)>,
    /// Approximate look-through holdings (ticker -> % of the fund).
    #[serde(default)]
    pub holdings: Vec<(String, f32)>,
}

#[derive(Deserialize)]
struct RawAsset {
    ticker: String,
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    vehicle: Vehicle,
    class: AssetClass,
    risk: u8,
    #[serde(default)]
    ter: Option<f32>,
    #[serde(default)]
    sectors: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    regions: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    holdings: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct RawFile {
    assets: Vec<RawAsset>,
}

fn map_to_vec(m: serde_json::Map<String, serde_json::Value>) -> Vec<(String, f32)> {
    m.into_iter()
        .filter_map(|(k, v)| v.as_f64().map(|f| (k, f as f32)))
        .collect()
}

impl From<RawAsset> for Asset {
    fn from(r: RawAsset) -> Self {
        Self {
            ticker: r.ticker.to_uppercase(),
            name: r.name,
            aliases: r.aliases,
            vehicle: r.vehicle,
            class: r.class,
            risk: r.risk.clamp(1, 5),
            ter: r.ter,
            sectors: map_to_vec(r.sectors),
            regions: map_to_vec(r.regions),
            holdings: map_to_vec(r.holdings),
        }
    }
}

pub struct AssetDb {
    assets: Vec<Asset>,
    index: HashMap<String, usize>,
}

/// Lower-case and keep only alphanumerics plus '&' so "S&P 500", "s&p500"
/// and "S & P 500" all collapse to the same key.
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '&')
        .flat_map(char::to_lowercase)
        .collect()
}

impl AssetDb {
    pub fn load(user_file: Option<&std::path::Path>) -> Self {
        let mut db = Self { assets: Vec::new(), index: HashMap::new() };
        if let Ok(f) = serde_json::from_str::<RawFile>(EMBEDDED) {
            db.extend(f.assets);
        }
        if let Some(p) = user_file {
            if let Ok(text) = std::fs::read_to_string(p) {
                match serde_json::from_str::<RawFile>(&text) {
                    Ok(f) => db.extend(f.assets),
                    Err(e) => eprintln!("ignoring {}: {e}", p.display()),
                }
            }
        }
        db
    }

    fn extend(&mut self, raw: Vec<RawAsset>) {
        for r in raw {
            let a: Asset = r.into();
            // A user entry with the same ticker replaces the embedded one.
            let idx = match self.index.get(&normalize(&a.ticker)) {
                Some(&i) if self.assets[i].ticker == a.ticker => {
                    self.assets[i] = a;
                    i
                }
                _ => {
                    self.assets.push(a);
                    self.assets.len() - 1
                }
            };
            let a = &self.assets[idx];
            let keys: Vec<String> = std::iter::once(&a.ticker)
                .chain(std::iter::once(&a.name))
                .chain(a.aliases.iter())
                .map(|s| normalize(s))
                .collect();
            for k in keys {
                if !k.is_empty() {
                    self.index.insert(k, idx);
                }
            }
        }
    }

    pub fn get(&self, ticker: &str) -> Option<&Asset> {
        self.index.get(&normalize(ticker)).map(|&i| &self.assets[i])
    }

    pub fn find(&self, phrase: &str) -> Option<&Asset> {
        self.get(phrase)
    }

    pub fn all(&self) -> &[Asset] {
        &self.assets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_db_parses_and_resolves_aliases() {
        let db = AssetDb::load(None);
        assert!(db.all().len() > 20);
        assert_eq!(db.find("NVIDIA").unwrap().ticker, "NVDA");
        assert_eq!(db.find("s&p 500").unwrap().ticker, "SPY");
        assert_eq!(db.find("vvsm").unwrap().ticker, "VVSM");
    }
}
