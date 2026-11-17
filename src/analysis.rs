//! Deterministic portfolio analysis. Every number the AI talks about comes
//! from here, so the model explains facts instead of inventing them.

use crate::assets::{Asset, AssetClass, AssetDb, Vehicle};
use crate::portfolio::{Holding, Portfolio};
use crate::profile::RiskLevel;
use std::collections::HashMap;
use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Good,
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub total: f32,
    pub classes: Vec<(String, f32)>,
    pub sectors: Vec<(String, f32)>,
    pub regions: Vec<(String, f32)>,
    /// Look-through exposure to individual companies / single assets.
    pub companies: Vec<(String, f32)>,
    pub effective_positions: f32,
    pub satellite_share: f32,
    pub risk_score: f32,
    pub revealed: Option<RiskLevel>,
    pub profile: Option<RiskLevel>,
    pub ter: Option<f32>,
    pub findings: Vec<Finding>,
    pub suggestions: Vec<String>,
    pub health: u8,
    pub proposal: Option<Portfolio>,
}

fn sorted(m: HashMap<String, f32>) -> Vec<(String, f32)> {
    let mut v: Vec<_> = m.into_iter().filter(|(_, x)| *x > 0.05).collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

/// A fund is "broad" when no single sector dominates it.
pub fn is_broad_equity_fund(a: &Asset) -> bool {
    a.vehicle == Vehicle::ETF
        && a.class == AssetClass::Equity
        && a.sectors.iter().all(|(_, w)| *w <= 40.0)
}

/// Single stocks, coins, sector/thematic funds and REITs are "satellites".
pub fn is_satellite(a: Option<&Asset>) -> bool {
    match a {
        None => true,
        Some(a) => match a.class {
            AssetClass::Crypto | AssetClass::RealEstate => true,
            AssetClass::Equity => !is_broad_equity_fund(a),
            _ => false,
        },
    }
}

fn pct(x: f32) -> String {
    format!("{x:.1}%")
}

pub fn analyse(p: &Portfolio, db: &AssetDb, profile: Option<RiskLevel>) -> Report {
    let mut r = Report { total: p.total(), profile, ..Default::default() };
    if p.is_empty() {
        return r;
    }
    // Analyse on a 100% basis; the unallocated part is reported separately.
    let scale = if r.total > 0.0 { 100.0 / r.total.max(100.0) } else { 1.0 };

    let mut classes: HashMap<String, f32> = HashMap::new();
    let mut sectors: HashMap<String, f32> = HashMap::new();
    let mut regions: HashMap<String, f32> = HashMap::new();
    let mut companies: HashMap<String, f32> = HashMap::new();
    let mut risk_sum = 0.0;
    let (mut ter_sum, mut ter_w) = (0.0, 0.0);
    let mut overlaps: Vec<String> = Vec::new();

    for h in &p.holdings {
        let w = h.percent * scale;
        let asset = db.get(&h.ticker);
        if is_satellite(asset) {
            r.satellite_share += w;
        }
        match asset {
            None => {
                *classes.entry("Unknown".into()).or_default() += w;
                *sectors.entry("Unclassified".into()).or_default() += w;
                *regions.entry("Unclassified".into()).or_default() += w;
                *companies.entry(h.ticker.clone()).or_default() += w;
                risk_sum += 4.0 * w;
            }
            Some(a) => {
                *classes.entry(a.class.label().into()).or_default() += w;
                for (s, sw) in &a.sectors {
                    *sectors.entry(s.clone()).or_default() += w * sw / 100.0;
                }
                for (g, gw) in &a.regions {
                    *regions.entry(g.clone()).or_default() += w * gw / 100.0;
                }
                match a.vehicle {
                    Vehicle::Stock | Vehicle::Coin => {
                        *companies.entry(a.ticker.clone()).or_default() += w;
                    }
                    Vehicle::ETF if a.class == AssetClass::Commodity => {
                        *companies.entry(a.ticker.clone()).or_default() += w;
                    }
                    _ => {}
                }
                for (t, hw) in &a.holdings {
                    *companies.entry(t.clone()).or_default() += w * hw / 100.0;
                }
                risk_sum += a.risk as f32 * w;
                if let Some(t) = a.ter {
                    ter_sum += t * w;
                    ter_w += w;
                }
            }
        }
    }

    // Overlap: the same company reached through more than one line.
    for h in &p.holdings {
        let Some(fund) = db.get(&h.ticker) else { continue };
        // Owning a big company directly while a broad index fund also holds a
        // sliver of it is normal; look-through exposure already counts it.
        if is_broad_equity_fund(fund) {
            continue;
        }
        for (t, hw) in &fund.holdings {
            if let Some(direct) = p.holdings.iter().find(|x| &x.ticker == t) {
                overlaps.push(format!(
                    "{} is held directly ({}) and again inside {} (~{:.0}% of that fund)",
                    t,
                    pct(direct.percent),
                    fund.ticker,
                    hw
                ));
            }
        }
    }
    for (i, a) in p.holdings.iter().enumerate() {
        for b in &p.holdings[i + 1..] {
            let (Some(fa), Some(fb)) = (db.get(&a.ticker), db.get(&b.ticker)) else { continue };
            if fa.vehicle != Vehicle::ETF || fb.vehicle != Vehicle::ETF || fa.class != fb.class {
                continue;
            }
            let shared: f32 = fa
                .holdings
                .iter()
                .filter_map(|(t, wa)| fb.holdings.iter().find(|(u, _)| u == t).map(|(_, wb)| wa.min(*wb)))
                .sum();
            let same_sector = fa.sectors.len() == 1 && fa.sectors == fb.sectors;
            if shared >= 10.0 || same_sector {
                overlaps.push(format!(
                    "{} and {} overlap strongly (≥{:.0}% of top holdings in common{})",
                    fa.ticker,
                    fb.ticker,
                    shared,
                    if same_sector { ", same sector" } else { "" }
                ));
            }
        }
    }

    r.classes = sorted(classes);
    r.sectors = sorted(sectors);
    r.regions = sorted(regions);
    r.companies = sorted(companies);
    let invested: f32 = p.holdings.iter().map(|h| h.percent * scale).sum();
    r.risk_score = if invested > 0.0 { risk_sum / invested } else { 0.0 };
    r.ter = (ter_w > 0.0).then(|| ter_sum / ter_w);

    // Concentration: Herfindahl index over single-name exposures. Broad funds'
    // remaining holdings are spread over hundreds of names (~0 contribution).
    let hhi: f32 = r.companies.iter().map(|(_, w)| (w / 100.0).powi(2)).sum();
    r.effective_positions = if hhi > 0.0 { (1.0 / hhi).min(500.0) } else { 500.0 };

    let top_company = r.companies.first().cloned();
    let top_sector = r.sectors.first().cloned();

    let mut revealed = r.risk_score;
    if let Some((_, w)) = &top_company {
        if *w > 20.0 {
            revealed += 0.5;
        }
    }
    if let Some((_, w)) = &top_sector {
        if *w > 50.0 {
            revealed += 0.5;
        }
    }
    r.revealed = Some(RiskLevel::from_level(revealed.round().clamp(1.0, 5.0) as u8));

    findings(&mut r, p, db, top_company, top_sector, overlaps);
    r.proposal = profile.map(|lvl| propose(p, db, lvl));
    r
}

fn findings(
    r: &mut Report,
    p: &Portfolio,
    db: &AssetDb,
    top_company: Option<(String, f32)>,
    top_sector: Option<(String, f32)>,
    overlaps: Vec<String>,
) {
    let f = &mut r.findings;
    let s = &mut r.suggestions;
    let mut add = |sev, title: String, detail: String| f.push(Finding { severity: sev, title, detail });
    let lvl = r.profile.unwrap_or(RiskLevel::Balanced);
    let class = |name: &str| r.classes.iter().find(|(c, _)| c == name).map_or(0.0, |x| x.1);

    // Allocation completeness.
    if r.total < 99.5 {
        add(
            Severity::Warning,
            format!("{} of the portfolio is not described", pct(100.0 - r.total)),
            "The percentages add up to less than 100%. Tell the advisor what the rest is (cash, another fund...) or press \"Fill rest with cash\".".into(),
        );
        s.push(format!("Describe the missing {} so the analysis covers the whole portfolio.", pct(100.0 - r.total)));
    } else if r.total > 100.5 {
        add(
            Severity::Warning,
            format!("Percentages add up to {}", pct(r.total)),
            "Results were rescaled to 100%. Use \"Normalize to 100%\" to fix the numbers.".into(),
        );
    }

    // Single-name concentration.
    let max_single = lvl.max_single_stock();
    if let Some((t, w)) = &top_company {
        let sources: Vec<String> = p
            .holdings
            .iter()
            .filter(|h| &h.ticker == t || db.get(&h.ticker).is_some_and(|a| a.holdings.iter().any(|(x, _)| x == t)))
            .map(|h| h.ticker.clone())
            .collect();
        if *w > max_single * 2.0 || *w > 35.0 {
            add(
                Severity::Critical,
                format!("{} is {} of everything you own", t, pct(*w)),
                format!(
                    "Counting what is inside your funds ({}), one company decides a large part of your result. A single bad year for {t} could cut your portfolio sharply. Suggested limit for your profile: {}.",
                    sources.join(", "),
                    pct(max_single)
                ),
            );
        } else if *w > max_single {
            add(
                Severity::Warning,
                format!("High exposure to {t}: {}", pct(*w)),
                format!("Look-through exposure via {}. Suggested limit for your profile: {}.", sources.join(", "), pct(max_single)),
            );
        }
        if *w > max_single {
            s.push(format!("Reduce total exposure to {t} from {} to at most {} (count the part held inside funds).", pct(*w), pct(max_single)));
        }
    }

    // Sector concentration.
    if let Some((sec, w)) = &top_sector {
        if !matches!(sec.as_str(), "Government Bonds" | "Corporate Bonds" | "Cash") {
            if *w > 60.0 {
                add(Severity::Critical, format!("{} of the portfolio is in {}", pct(*w), sec),
                    format!("Sectors move together. {sec} has historically had drawdowns of 50%+ (e.g. semiconductors fell ~65% in 2000-02 and ~45% in 2022)."));
                s.push(format!("Bring {sec} below ~30-40% by adding other sectors through a broad fund."));
            } else if *w > 40.0 {
                add(Severity::Warning, format!("Heavy tilt to {}: {}", sec, pct(*w)),
                    "A broad market fund holds roughly 10-30% in its largest sector.".into());
            }
        }
    }

    // Regions.
    if let Some((reg, w)) = r.regions.first() {
        if reg != "Global" && reg != "Unclassified" && *w > 85.0 && class("Equities") > 50.0 {
            add(Severity::Info, format!("{} of your money depends on {}", pct(*w), reg),
                "Home or single-region bias. A global fund spreads the bet across US, Europe, Asia and emerging markets.".into());
        }
    }

    // Number of effective positions.
    let n = r.effective_positions;
    if n < 5.0 {
        add(Severity::Warning, format!("Behaves like only {n:.1} independent positions"),
            "Your result depends on very few names. Diversified portfolios behave like dozens or hundreds of positions.".into());
    } else if n >= 20.0 {
        add(Severity::Good, "Well diversified across companies".into(),
            format!("Single-name risk is spread out (≈{} effective positions).", if n >= 500.0 { "500+".into() } else { format!("{n:.0}") }));
    }

    // Core-satellite structure.
    let has_core = p.holdings.iter().any(|h| db.get(&h.ticker).is_some_and(is_broad_equity_fund));
    if has_core {
        add(Severity::Good, "Has a broad, diversified core".into(), "A broad index fund is the most robust base for long-term growth.".into());
    } else if class("Equities") > 30.0 {
        add(Severity::Warning, "No diversified core".into(),
            "Everything is a bet on specific companies or themes. Most long-term results come from a low-cost global core.".into());
        s.push("Build a core of a broad global equity ETF (e.g. VWCE or IWDA) as the largest position.".into());
    }
    let sat_max = lvl.max_satellite();
    if r.satellite_share > sat_max + 0.5 {
        add(
            if r.satellite_share > sat_max * 2.0 { Severity::Critical } else { Severity::Warning },
            format!("{} in single stocks / themes / crypto", pct(r.satellite_share)),
            format!("For a {} investor a satellite share up to ~{} is a common ceiling.", lvl.label().to_lowercase(), pct(sat_max)),
        );
        s.push(format!("Trim satellite positions (single stocks, sector ETFs, crypto) from {} to about {}.", pct(r.satellite_share), pct(sat_max)));
    }

    // Overlap.
    if overlaps.is_empty() {
        if p.holdings.len() > 1 {
            add(Severity::Good, "No obvious overlap between positions".into(), String::new());
        }
    } else if top_company.as_ref().is_some_and(|(_, w)| *w > max_single) {
        add(Severity::Warning, "Overlapping positions (double counting)".into(), overlaps.join(".\n"));
        s.push("Merge overlapping positions: they look like diversification but are the same bet twice.".into());
    } else {
        add(
            Severity::Info,
            "Some companies are held twice".into(),
            format!("{}. Total exposure stays within your limit, so this is a deliberate tilt, not a problem.", overlaps.join(".\n")),
        );
    }

    // Defensive assets vs profile.
    let (eq_t, bd_t, cs_t) = lvl.target_mix();
    let bonds = class("Bonds");
    let defensive = bonds + class("Cash") + class("Commodities");
    if r.profile.is_some() && bd_t > 0.0 && bonds + 10.0 < bd_t {
        add(Severity::Warning, format!("Bonds {} vs ~{} typical for your profile", pct(bonds), pct(bd_t)),
            "Bonds cushion stock-market falls and let you rebalance (buy stocks cheap) during crashes.".into());
        s.push(format!("Add roughly {} in a broad bond ETF (e.g. AGGH) or short-term government bonds.", pct(bd_t - bonds)));
    }
    if defensive < 0.5 && lvl <= RiskLevel::Growth {
        add(Severity::Info, "No defensive assets".into(), "There is nothing in the portfolio that tends to hold up in a stock-market crash.".into());
    }
    let equity_like = class("Equities") + class("Crypto") + class("Real estate");
    if r.profile.is_some() && equity_like > eq_t + 15.0 {
        s.push(format!("Risky assets are {} vs ~{} for your profile: shift the difference to bonds/cash.", pct(equity_like), pct(eq_t)));
    }
    if r.profile.is_some() && class("Cash") > cs_t + 20.0 && lvl >= RiskLevel::Balanced {
        add(Severity::Info, format!("{} in cash", pct(class("Cash"))),
            "Large cash holdings lose purchasing power to inflation over long horizons.".into());
    }

    // Crypto.
    let crypto = class("Crypto");
    if crypto > 10.0 {
        add(Severity::Warning, format!("Crypto at {}", pct(crypto)),
            "Crypto has fallen 70-80% several times. Many guidelines keep it to 1-5% of a portfolio.".into());
    }

    // Costs.
    if let Some(t) = r.ter {
        if t > 0.4 {
            add(Severity::Info, format!("Average fund cost {t:.2}%/year"), "Cheaper broad funds cost 0.07-0.22%/year; costs compound against you.".into());
        } else {
            add(Severity::Good, format!("Low average fund cost ({t:.2}%/year)"), String::new());
        }
    }

    // Unknown assets.
    let unknown: Vec<&Holding> = p.holdings.iter().filter(|h| db.get(&h.ticker).is_none()).collect();
    if !unknown.is_empty() {
        add(Severity::Info, "Some assets are not in the local database".into(),
            format!("{} were treated as single high-risk positions. Ask the advisor about them or add them to assets.json.",
                unknown.iter().map(|h| h.ticker.as_str()).collect::<Vec<_>>().join(", ")));
    }

    // Behaviour vs. stated profile.
    if let (Some(prof), Some(rev)) = (r.profile, r.revealed) {
        let diff = rev.level() as i8 - prof.level() as i8;
        if diff >= 2 {
            add(Severity::Critical, format!("Portfolio is {} but you are {}", rev.label(), prof.label()),
                "The portfolio takes much more risk than your answers say you can handle. This mismatch is the most common reason people sell at the bottom.".into());
        } else if diff == 1 {
            add(Severity::Warning, format!("Portfolio slightly riskier than your profile ({} vs {})", rev.label(), prof.label()), String::new());
        } else if diff <= -2 {
            add(Severity::Info, format!("Portfolio is more cautious than your profile ({} vs {})", rev.label(), prof.label()),
                "You could accept more risk for higher expected long-term growth.".into());
        } else {
            add(Severity::Good, format!("Risk level matches your profile ({})", prof.label()), String::new());
        }
    } else if r.profile.is_none() {
        s.push("Complete the investor profile so suggestions can be matched to your risk tolerance.".into());
    }

    let mut health: i32 = 100;
    for x in f.iter() {
        health -= match x.severity {
            Severity::Critical => 18,
            Severity::Warning => 8,
            Severity::Info => 2,
            Severity::Good => 0,
        };
    }
    r.health = health.clamp(0, 100) as u8;
    f.sort_by(|a, b| b.severity.cmp(&a.severity));
}

/// Build a concrete core-satellite proposal that respects the profile's
/// limits while keeping the user's own convictions where possible.
pub fn propose(p: &Portfolio, db: &AssetDb, lvl: RiskLevel) -> Portfolio {
    let (eq_t, bd_t, cs_t) = lvl.target_mix();
    let sat_cap = lvl.max_satellite().min(eq_t * 0.4);
    let single_cap = lvl.max_single_stock();

    let mut core = Vec::new();
    let mut sats = Vec::new();
    let mut bonds = Vec::new();
    let mut defensive = Vec::new();
    for h in &p.holdings {
        let a = db.get(&h.ticker);
        match a.map(|a| a.class) {
            Some(AssetClass::Bonds) => bonds.push(h.clone()),
            Some(AssetClass::Cash | AssetClass::Commodity) => defensive.push(h.clone()),
            _ if is_satellite(a) => sats.push(h.clone()),
            _ => core.push(h.clone()),
        }
    }

    // Satellites: keep the user's picks, scaled into the allowed budget.
    let sat_sum: f32 = sats.iter().map(|h| h.percent).sum();
    let sat_factor = if sat_sum > sat_cap { sat_cap / sat_sum } else { 1.0 };
    for h in &mut sats {
        h.percent *= sat_factor;
        let is_stock = db.get(&h.ticker).is_none_or(|a| a.vehicle != Vehicle::ETF);
        if is_stock {
            h.percent = h.percent.min(single_cap);
        }
    }
    let sat_used: f32 = sats.iter().map(|h| h.percent).sum();

    let mut out = Portfolio::default();
    let fill = |out: &mut Portfolio, group: &[Holding], budget: f32, default: &str| {
        if budget <= 0.05 {
            return;
        }
        let total: f32 = group.iter().map(|h| h.percent).sum();
        if total > 0.0 {
            for h in group {
                out.add(Holding { ticker: h.ticker.clone(), percent: budget * h.percent / total, known: h.known });
            }
        } else {
            out.add(Holding { ticker: default.into(), percent: budget, known: true });
        }
    };
    fill(&mut out, &core, eq_t - sat_used, "VWCE");
    for h in sats {
        out.add(h);
    }
    fill(&mut out, &bonds, bd_t, "AGGH");
    fill(&mut out, &defensive, cs_t, "CASH");

    for h in &mut out.holdings {
        h.percent = (h.percent * 2.0).round() / 2.0;
    }
    out.holdings.retain(|h| h.percent > 0.0);
    // Put any rounding residue on the largest line.
    let diff = 100.0 - out.total();
    out.sort();
    if let Some(first) = out.holdings.first_mut() {
        first.percent += diff;
    }
    out
}

/// Plain-text report used as grounding context for the AI and for export.
pub fn to_text(r: &Report, p: &Portfolio) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "PORTFOLIO: {}", p.describe());
    let _ = writeln!(s, "Health score: {}/100", r.health);
    let list = |v: &[(String, f32)], n: usize| v.iter().take(n).map(|(k, w)| format!("{k} {w:.1}%")).collect::<Vec<_>>().join(", ");
    let _ = writeln!(s, "Asset classes: {}", list(&r.classes, 8));
    let _ = writeln!(s, "Sectors (look-through): {}", list(&r.sectors, 6));
    let _ = writeln!(s, "Regions: {}", list(&r.regions, 5));
    let _ = writeln!(s, "Largest single-name exposures (incl. inside funds): {}", list(&r.companies, 6));
    let _ = writeln!(s, "Effective number of positions: {:.1}", r.effective_positions);
    let _ = writeln!(s, "Satellite share (single stocks/themes/crypto): {:.1}%", r.satellite_share);
    let _ = writeln!(s, "Weighted volatility class (1-5): {:.2}", r.risk_score);
    if let Some(rv) = r.revealed {
        let _ = writeln!(s, "Risk level implied by the portfolio: {}", rv.label());
    }
    if let Some(t) = r.ter {
        let _ = writeln!(s, "Average fund cost: {t:.2}%/yr");
    }
    let _ = writeln!(s, "FINDINGS:");
    for f in &r.findings {
        let _ = writeln!(s, "- [{:?}] {}{}", f.severity, f.title, if f.detail.is_empty() { String::new() } else { format!(": {}", f.detail.replace('\n', " ")) });
    }
    if !r.suggestions.is_empty() {
        let _ = writeln!(s, "RULE-BASED SUGGESTIONS:");
        for x in &r.suggestions {
            let _ = writeln!(s, "- {x}");
        }
    }
    if let Some(pp) = &r.proposal {
        let _ = writeln!(s, "RULE-BASED PROPOSAL: {}", pp.describe());
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::parse;

    #[test]
    fn nvidia_plus_semis_etf_is_flagged() {
        let db = AssetDb::load(None);
        let p = parse(&db, "I have 25% NVIDIA and 50% VVSM ETF").portfolio;
        let r = analyse(&p, &db, Some(RiskLevel::Balanced));
        let titles: Vec<&str> = r.findings.iter().map(|f| f.title.as_str()).collect();
        assert!(titles.iter().any(|t| t.contains("not described")), "{titles:?}");
        assert!(titles.iter().any(|t| t.contains("Overlapping")), "{titles:?}");
        assert!(titles.iter().any(|t| t.contains("Semiconductors")), "{titles:?}");
        assert_eq!(r.sectors[0].0, "Semiconductors");
        // NVDA: 25 direct + 10% of 50 inside VVSM = 30.
        assert_eq!(r.companies[0].0, "NVDA");
        assert!((r.companies[0].1 - 30.0).abs() < 0.01);
        assert!(r.health < 50);
    }

    #[test]
    fn proposal_sums_to_100_and_respects_caps() {
        let db = AssetDb::load(None);
        let p = parse(&db, "25% NVIDIA, 50% VVSM, 25% cash").portfolio;
        let pp = propose(&p, &db, RiskLevel::Balanced);
        assert!((pp.total() - 100.0).abs() < 0.01, "{}", pp.describe());
        let nv = pp.holdings.iter().find(|h| h.ticker == "NVDA").unwrap();
        assert!(nv.percent <= RiskLevel::Balanced.max_single_stock());
        assert!(pp.holdings.iter().any(|h| h.ticker == "VWCE"));
        assert!(pp.holdings.iter().any(|h| h.ticker == "AGGH"));
    }

    #[test]
    fn global_fund_is_healthy() {
        let db = AssetDb::load(None);
        let p = parse(&db, "70% vwce 25% aggh 5% cash").portfolio;
        let r = analyse(&p, &db, Some(RiskLevel::Balanced));
        assert!(r.findings.iter().all(|f| f.severity != Severity::Critical), "{:?}", r.findings);
        assert!(r.health >= 85, "{}", r.health);
    }
}
