"""
LoRA fine-tune of Qwen3-4B for Personal Portfolio Manager, using Unsloth.
Runs on a FREE Google Colab T4 or Kaggle GPU (~30-60 min for 2k examples).

    pip install unsloth
    python train.py --data dataset.jsonl --out outputs

Produces outputs/portfolio-analyst-Q4_K_M.gguf for Ollama (see README.md).
"""
import argparse

from datasets import load_dataset
from trl import SFTConfig, SFTTrainer
from unsloth import FastLanguageModel
from unsloth.chat_templates import train_on_responses_only

ap = argparse.ArgumentParser()
ap.add_argument("--data", default="dataset.jsonl")
ap.add_argument("--out", default="outputs")
ap.add_argument("--base", default="unsloth/Qwen3-4B-Instruct-2507")
ap.add_argument("--epochs", type=float, default=2)
args = ap.parse_args()

model, tokenizer = FastLanguageModel.from_pretrained(
    model_name=args.base,
    max_seq_length=4096,
    load_in_4bit=True,  # QLoRA: fits in a free 16 GB GPU
)
model = FastLanguageModel.get_peft_model(
    model,
    r=16,
    lora_alpha=16,
    lora_dropout=0,
    target_modules=["q_proj", "k_proj", "v_proj", "o_proj", "gate_proj", "up_proj", "down_proj"],
    use_gradient_checkpointing="unsloth",
)

ds = load_dataset("json", data_files=args.data, split="train")
ds = ds.map(lambda ex: {"text": tokenizer.apply_chat_template(ex["messages"], tokenize=False)})
split = ds.train_test_split(test_size=0.05, seed=42)

trainer = SFTTrainer(
    model=model,
    tokenizer=tokenizer,
    train_dataset=split["train"],
    eval_dataset=split["test"],
    args=SFTConfig(
        dataset_text_field="text",
        per_device_train_batch_size=2,
        gradient_accumulation_steps=4,
        num_train_epochs=args.epochs,
        learning_rate=2e-4,
        lr_scheduler_type="cosine",
        warmup_ratio=0.03,
        logging_steps=10,
        eval_strategy="epoch",
        output_dir=args.out,
        report_to="none",
    ),
)
# Learn only the assistant answers, not the long system prompt.
trainer = train_on_responses_only(
    trainer,
    instruction_part="<|im_start|>user\n",
    response_part="<|im_start|>assistant\n",
)
trainer.train()

model.save_pretrained_gguf(f"{args.out}/portfolio-analyst", tokenizer, quantization_method="q4_k_m")
print("Done. GGUF written under", args.out)
