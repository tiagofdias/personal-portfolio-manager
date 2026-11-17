#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai;
mod analysis;
mod app;
mod theme;
mod assets;
mod dataset;
mod portfolio;
mod profile;
mod storage;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        // Headless helpers (no window is created).
        Some("--report") => {
            let db = assets::AssetDb::load(None);
            let text = args[1..].join(" ");
            let p = portfolio::parse(&db, &text).portfolio;
            let level = args.iter().find_map(|a| a.strip_prefix("--level=")).and_then(|l| l.parse().ok()).map(profile::RiskLevel::from_level);
            print!("{}", analysis::to_text(&analysis::analyse(&p, &db, level.or(Some(profile::RiskLevel::Balanced))), &p));
            return Ok(());
        }
        Some("--chat") => {
            // --chat "<portfolio text>" "<question>" [--model=NAME]
            let db = assets::AssetDb::load(None);
            let p = portfolio::parse(&db, args.get(1).map_or("", String::as_str)).portfolio;
            let question = args.get(2).cloned().unwrap_or_else(|| "Analyse my portfolio.".into());
            let mut settings = storage::load().ai;
            if let Some(m) = args.iter().find_map(|a| a.strip_prefix("--model=")) {
                settings.model = m.into();
            }
            let mut prof = profile::Profile::default();
            prof.ensure_len();
            prof.answers = vec![Some(3), Some(2), Some(2), Some(3), Some(3), Some(2), Some(2)];
            let r = analysis::analyse(&p, &db, prof.risk_level());
            let system = ai::build_system(&prof, &analysis::to_text(&r, &p));
            let history = [ai::ChatMessage { role: ai::Role::User, content: question }];
            let job = ai::start_chat(&settings, system, &history, || {});
            let mut full = String::new();
            for ev in job.rx.iter() {
                match ev {
                    ai::AiEvent::Token(t) => {
                        print!("{t}");
                        full.push_str(&t);
                        use std::io::Write;
                        let _ = std::io::stdout().flush();
                    }
                    ai::AiEvent::Done => break,
                    ai::AiEvent::Error(e) => {
                        eprintln!("\nERROR: {e}");
                        break;
                    }
                }
            }
            let clean = ai::strip_thinking(&full);
            match ai::extract_proposal(&clean) {
                Some(b) => println!("\n\n[proposal parsed] {}", portfolio::parse_block(&db, &b).describe()),
                None => println!("\n\n[no <portfolio> block]"),
            }
            return Ok(());
        }
        Some("--gen-dataset") => {
            let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(500);
            let out = args.get(2).cloned().unwrap_or_else(|| "ai/finetune/dataset.jsonl".into());
            match dataset::generate(n, &out) {
                Ok(()) => println!("wrote {n} examples to {out}"),
                Err(e) => eprintln!("error: {e}"),
            }
            return Ok(());
        }
        _ => {}
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Personal Portfolio Manager")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([760.0, 520.0]),
        multisampling: 0,
        depth_buffer: 0,
        stencil_buffer: 0,
        ..Default::default()
    };
    eframe::run_native("Personal Portfolio Manager", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
