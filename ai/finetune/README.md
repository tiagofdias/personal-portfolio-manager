# Fine-tuning the free portfolio analyst

Everything here is free: an Apache-2.0 base model (Qwen3-4B), free GPU time
(Google Colab or Kaggle), open-source tools (Unsloth, TRL) and local
inference with Ollama.

## How the AI is designed

The AI does **not** calculate anything. The Rust engine (`src/analysis.rs`)
computes all facts (look-through exposure, overlap, concentration, risk
level, profile fit, a rule-based proposal) and passes them to the model in the
system prompt. The model's job is to:

1. explain the facts in plain language, ranked by importance,
2. understand the investor (questionnaire + what they say in the chat),
3. negotiate improvements and output them as a `<portfolio>` block the app
   can apply with one click,
4. remember durable facts about the person through `<note>` tags.

Because the facts come from code, a small model (4B parameters, ~2.5 GB of RAM)
is enough. Fine-tuning mainly teaches it the format, the tone, and to stay
grounded.

## Steps

1. **Generate training data** from the engine (same prompt format the app uses):

   ```bash
   cargo run --release -- --gen-dataset 2000 ai/finetune/dataset.jsonl
   ```

2. **Improve quality (the step that matters most).** The generated answers
   are templates. Review and rewrite a few hundred of them by hand, or with
   the best free model you can run, so they sound like a great human advisor.
   Add real multi-turn conversations too: the user pushes back ("I don't want
   to sell NVIDIA"), adds context ("I'm 55"), and the assistant adapts and
   emits a new `<portfolio>` block. 300-1,000 high-quality examples beat
   10,000 mediocre ones.

3. **Train** on a free Colab/Kaggle T4 GPU:

   ```bash
   pip install unsloth
   python train.py --data dataset.jsonl --out outputs
   ```

4. **Install locally:** copy `outputs/portfolio-analyst*.gguf` next to
   `ai/Modelfile`, change its `FROM` line to that file, then:

   ```bash
   ollama create portfolio-analyst -f ai/Modelfile
   ```

   The app uses the model named `portfolio-analyst` by default.

5. **Evaluate** before replacing the old model: run 30-50 held-out
   portfolios through both models and check that numbers match the FACTS
   block, that advice respects the profile, and that `<portfolio>` blocks
   add up to 100%.

## Sharing

Publish the GGUF and the dataset on Hugging Face under Apache-2.0 so every
user can download it for free (`ollama pull hf.co/<you>/portfolio-analyst`).
