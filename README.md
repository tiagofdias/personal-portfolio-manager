# Personal Portfolio Manager

A free, open-source native desktop app (Rust, no webview) that:

1. reads your portfolio written in plain words with **percentages only**:
   *"I have 25% NVIDIA and 50% VVSM ETF, the rest in cash"*;
2. profiles **what kind of investor you are** (7-question risk questionnaire,
   plus what the portfolio itself reveals);
3. analyses the **good and bad points**: look-through exposure inside ETFs,
   overlap/double counting, single-company and sector concentration, regions,
   costs, defensive assets, and whether the risk fits you;
4. proposes improvements, then lets you **refine the plan with a free local AI**.
   The AI sees the facts and your profile, remembers what you tell it about
   yourself, and proposes new allocations you can apply with one click.

> Educational tool, not personal financial advice. Nobody can guarantee
> returns; the app aims for the best **risk-adjusted fit** for you.

## Architecture

```
src/
  portfolio.rs  natural-language % parser + portfolio model
  assets.rs     embedded asset DB (data/assets.json) + O(1) alias lookup
  profile.rs    questionnaire, risk level, per-profile limits
  analysis.rs   deterministic engine: exposures, overlap, findings, proposal
  ai.rs         streaming client for any OpenAI-compatible endpoint (Ollama)
  app.rs        egui UI (OpenGL, repaints only on input -> ~0% CPU idle)
  storage.rs    JSON state in %APPDATA%/PersonalPortfolioManager
  dataset.rs    synthetic fine-tuning data generator
ai/
  Modelfile     Ollama model "portfolio-analyst" (Qwen3-4B, Apache-2.0)
  finetune/     free LoRA fine-tuning kit (Unsloth on free Colab/Kaggle GPU)
```

Facts come from code; the model only explains and negotiates. That keeps the
analysis consistent and lets a small free model do the job well.

## Run

Requirements: Rust 1.95+.

```bash
cargo run --release
```

Free AI (optional, the analysis works without it):

```bash
ollama pull qwen3:4b
```

```bash
ollama create portfolio-analyst -f ai/Modelfile
```

Install Ollama from ollama.com first. Any OpenAI-compatible server
(llama.cpp `llama-server`, LM Studio...) also works: change the endpoint in
**Settings**.

Headless helpers:

```bash
cargo run -- --report "25% NVIDIA, 50% VVSM, 25% cash" --level=3
```

```bash
cargo run --release -- --gen-dataset 2000 ai/finetune/dataset.jsonl
```

Windows note: if linking fails with `LNK1104`, the project path is too long;
set a short `CARGO_TARGET_DIR` (e.g. `C:\t\ppm`).

## Extending the asset database

`data/assets.json` holds ~30 common stocks/ETFs with **approximate** sector,
region and top-holding weights. Drop an `assets.json` with the same format in
the app's data folder to add or correct assets without rebuilding.

## Tests

```bash
cargo test
```
