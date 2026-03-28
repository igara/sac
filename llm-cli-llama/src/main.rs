use std::{
    io::{self, Write},
    num::NonZeroU32,
    path::PathBuf,
};

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use hf_hub::{api::sync::Api, Repo, RepoType};
use rustyline::{error::ReadlineError, DefaultEditor};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
#[allow(deprecated)]
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel, LlamaChatMessage, Special},
    sampling::LlamaSampler,
};

// ANSI color codes
const GREEN: &str = "\x1b[32m";
const CYAN: &str  = "\x1b[36m";
const BOLD: &str  = "\x1b[1m";
const RESET: &str = "\x1b[0m";

// ── Session persistence ───────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
struct SavedMessage {
    role: String,
    content: String,
}

#[derive(Serialize, Deserialize)]
struct SavedSession {
    name: String,
    created_at: String,
    messages: Vec<SavedMessage>,
}

/// ~/.local/share/llm-llama/sessions/ が存在しない場合は作成して返す
fn sessions_dir() -> Result<PathBuf> {
    let base = dirs_next().unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("sessions");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create sessions directory: {}", dir.display()))?;
    Ok(dir)
}

fn dirs_next() -> Option<PathBuf> {
    // ~/.local/share/llm-llama
    dirs::data_local_dir().map(|d| d.join("llm-llama"))
}

fn save_session(name: &str, messages: &[SavedMessage]) -> Result<PathBuf> {
    let dir = sessions_dir()?;
    let safe_name: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let file = dir.join(format!("{safe_name}.json"));
    let session = SavedSession {
        name: name.to_owned(),
        created_at: chrono_now(),
        messages: messages.to_vec(),
    };
    let json = serde_json::to_string_pretty(&session).context("Failed to serialize session")?;
    std::fs::write(&file, json)
        .with_context(|| format!("Failed to write session to {}", file.display()))?;
    Ok(file)
}

fn load_session(name: &str) -> Result<SavedSession> {
    let dir = sessions_dir()?;
    let safe_name: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let file = dir.join(format!("{safe_name}.json"));
    let json = std::fs::read_to_string(&file)
        .with_context(|| format!("Session '{}' not found", name))?;
    serde_json::from_str(&json).context("Failed to parse session file")
}

fn list_sessions() -> Result<Vec<(String, String)>> {
    // (ファイル名stem, created_at) のリストを返す（新しい順）
    let dir = sessions_dir()?;
    let mut entries: Vec<(String, String)> = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let created_at = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .and_then(|v| v["created_at"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                entries.push((stem.to_owned(), created_at));
            }
        }
    }
    entries.sort_by(|a, b| b.1.cmp(&a.1)); // 新しい順
    Ok(entries)
}

fn delete_session(name: &str) -> Result<()> {
    let dir = sessions_dir()?;
    let safe_name: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let file = dir.join(format!("{safe_name}.json"));
    std::fs::remove_file(&file)
        .with_context(|| format!("Failed to delete session '{}'", name))
}

/// `/sessions` コマンド: ↑↓ で選択してセッション名を返す。キャンセルは None。
fn select_session_interactive() -> Result<Option<String>> {
    let sessions = list_sessions()?;
    if sessions.is_empty() {
        return Ok(None);
    }
    let items: Vec<String> = sessions
        .iter()
        .map(|(name, ts)| {
            if ts.is_empty() {
                name.clone()
            } else {
                format!("{name}  [{ts}]")
            }
        })
        .collect();

    use dialoguer::Select;
    let selection = Select::new()
        .with_prompt("Select session (↑↓ to move, Enter to load, Esc to cancel)")
        .items(&items)
        .default(0)
        .interact_opt()
        .context("Failed to display session selector")?;

    Ok(selection.map(|i| sessions[i].0.clone()))
}

/// セッションの会話履歴をページャー形式で標準エラーに出力する
fn print_session_review(sess: &SavedSession) {
    eprintln!();
    eprintln!("┏{}", "━".repeat(72));
    eprintln!("┃ Session : {}", sess.name);
    eprintln!("┃ Saved   : {}", sess.created_at);
    eprintln!("┃ Messages: {}", sess.messages.iter().filter(|m| m.role != "system").count());
    eprintln!("┗{}", "━".repeat(72));
    for msg in &sess.messages {
        match msg.role.as_str() {
            "system" => {
                eprintln!("\n\x1b[2m[System]\x1b[0m");
                eprintln!("\x1b[2m{}\x1b[0m", msg.content);
            }
            "user" => {
                eprintln!("\n\x1b[1m\x1b[32mYou:\x1b[0m");
                eprintln!("{}", msg.content);
            }
            "assistant" => {
                eprintln!("\n\x1b[1m\x1b[36mAssistant:\x1b[0m");
                eprintln!("\x1b[36m{}\x1b[0m", msg.content);
            }
            _ => {}
        }
    }
    eprintln!("\n{}", "─".repeat(72));
    eprintln!();
}

/// `/review` セッション選択 UI
fn select_session_for_review() -> Result<Option<String>> {
    let sessions = list_sessions()?;
    if sessions.is_empty() {
        return Ok(None);
    }
    let items: Vec<String> = sessions
        .iter()
        .map(|(name, ts)| {
            if ts.is_empty() { name.clone() } else { format!("{name}  [{ts}]") }
        })
        .collect();

    use dialoguer::Select;
    let selection = Select::new()
        .with_prompt("Review session (↑↓ to move, Enter to view, Esc to cancel)")
        .items(&items)
        .default(0)
        .interact_opt()
        .context("Failed to display session selector")?;

    Ok(selection.map(|i| sessions[i].0.clone()))
}

/// `/delete` コマンド: ↑↓ で選択してセッション名を返す。キャンセルは None。
fn select_session_for_delete() -> Result<Option<String>> {
    let sessions = list_sessions()?;
    if sessions.is_empty() {
        return Ok(None);
    }
    let items: Vec<String> = sessions
        .iter()
        .map(|(name, ts)| {
            if ts.is_empty() { name.clone() } else { format!("{name}  [{ts}]") }
        })
        .collect();

    use dialoguer::Select;
    let selection = Select::new()
        .with_prompt("Delete session (↑↓ to move, Enter to delete, Esc to cancel)")
        .items(&items)
        .default(0)
        .interact_opt()
        .context("Failed to display session selector")?;

    Ok(selection.map(|i| sessions[i].0.clone()))
}

fn chrono_now() -> String {
    // 簡易タイムスタンプ（std のみ）
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // epoch → YYYY-MM-DD HH:MM (UTC, 簡易実装)
    let s = secs;
    let minutes = s / 60 % 60;
    let hours = s / 3600 % 24;
    let days = s / 86400;
    let years = 1970 + days / 365;
    let day_of_year = days % 365;
    let month = day_of_year / 30 + 1;
    let day = day_of_year % 30 + 1;
    format!("{:04}-{:02}-{:02} {:02}:{:02}", years, month.min(12), day.min(31), hours, minutes)
}


/// Preset GGUF models (Q4_K_M quantization, auto-downloaded from HuggingFace)
#[derive(Debug, Clone, ValueEnum)]
enum Preset {
    // ── 英語汎用 ──────────────────────────────────────────────────────────
    /// Llama 3.2 1B Instruct  (~0.7 GB)
    #[value(name = "llama32-1b")]
    Llama321b,
    /// Llama 3.2 3B Instruct  (~1.9 GB)
    #[value(name = "llama32-3b")]
    Llama323b,
    /// Llama 3.1 8B Instruct  (~4.7 GB)
    #[value(name = "llama31-8b")]
    Llama318b,
    /// Mistral 7B Instruct v0.3  (~4.1 GB)
    #[value(name = "mistral-7b")]
    Mistral7b,
    /// Qwen 2.5 7B Instruct  (~4.4 GB)
    #[value(name = "qwen25-7b")]
    Qwen257b,
    /// Gemma 3 1B Instruct  (~0.8 GB)
    #[value(name = "gemma3-1b")]
    Gemma31b,
    /// Gemma 3 4B Instruct  (~2.5 GB)
    #[value(name = "gemma3-4b")]
    Gemma34b,
    // ── 日本語特化 ────────────────────────────────────────────────────────
    /// [JP] Llama 3.1 Swallow 8B Instruct v0.3  (~4.9 GB)  日本語+英語
    #[value(name = "swallow-8b")]
    Swallow8b,
    /// [JP] ELYZA Llama-3 JP 8B Instruct  (~4.9 GB)  日本語重点
    #[value(name = "elyza-jp-8b")]
    ElyzaJp8b,
    /// [JP] CyberAgent calm3 22B Chat  (~13 GB)  日本語特化 大規模
    #[value(name = "calm3-22b")]
    Calm322b,
    /// [JP] Google Gemma 2 2B Japanese  (~1.4 GB)  Google公式日本語特化
    #[value(name = "gemma2-jpn-2b")]
    Gemma2Jpn2b,
    /// [JP] Google Gemma 3 12B Instruct  (~7.0 GB)  多言語・日本語高品質
    #[value(name = "gemma3-12b")]
    Gemma312b,
}

impl Preset {
    fn repo_id(&self) -> &'static str {
        match self {
            Self::Llama321b  => "bartowski/Llama-3.2-1B-Instruct-GGUF",
            Self::Llama323b  => "bartowski/Llama-3.2-3B-Instruct-GGUF",
            Self::Llama318b  => "bartowski/Meta-Llama-3.1-8B-Instruct-GGUF",
            Self::Mistral7b  => "bartowski/Mistral-7B-Instruct-v0.3-GGUF",
            Self::Qwen257b   => "bartowski/Qwen2.5-7B-Instruct-GGUF",
            Self::Gemma31b   => "bartowski/gemma-3-1b-it-GGUF",
            Self::Gemma34b   => "bartowski/gemma-3-4b-it-GGUF",
            Self::Swallow8b  => "mmnga/tokyotech-llm-Llama-3.1-Swallow-8B-Instruct-v0.3-gguf",
            Self::ElyzaJp8b  => "mmnga/elyza-Llama-3-ELYZA-JP-8B-GGUF",
            Self::Calm322b   => "mmnga/cyberagent-calm3-22b-chat-gguf",
            Self::Gemma2Jpn2b => "bartowski/gemma-2-2b-jpn-it-GGUF",
            Self::Gemma312b  => "bartowski/gemma-3-12b-it-GGUF",
        }
    }

    fn filename(&self) -> &'static str {
        match self {
            Self::Llama321b  => "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
            Self::Llama323b  => "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
            Self::Llama318b  => "Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf",
            Self::Mistral7b  => "Mistral-7B-Instruct-v0.3-Q4_K_M.gguf",
            Self::Qwen257b   => "Qwen2.5-7B-Instruct-Q4_K_M.gguf",
            Self::Gemma31b   => "gemma-3-1b-it-Q4_K_M.gguf",
            Self::Gemma34b   => "gemma-3-4b-it-Q4_K_M.gguf",
            Self::Swallow8b  => "tokyotech-llm-Llama-3.1-Swallow-8B-Instruct-v0.3-q4_K_M.gguf",
            Self::ElyzaJp8b  => "elyza-Llama-3-ELYZA-JP-8B-q4_K_M.gguf",
            Self::Calm322b   => "cyberagent-calm3-22b-chat-q4_K_M.gguf",
            Self::Gemma2Jpn2b => "gemma-2-2b-jpn-it-Q4_K_M.gguf",
            Self::Gemma312b  => "gemma-3-12b-it-Q4_K_M.gguf",
        }
    }

    fn size(&self) -> &'static str {
        match self {
            Self::Llama321b  => "~0.7 GB",
            Self::Llama323b  => "~1.9 GB",
            Self::Llama318b  => "~4.7 GB",
            Self::Mistral7b  => "~4.1 GB",
            Self::Qwen257b   => "~4.4 GB",
            Self::Gemma31b   => "~0.8 GB",
            Self::Gemma34b   => "~2.5 GB",
            Self::Swallow8b  => "~4.9 GB",
            Self::ElyzaJp8b  => "~4.9 GB",
            Self::Calm322b   => "~13 GB",
            Self::Gemma2Jpn2b => "~1.4 GB",
            Self::Gemma312b  => "~7.0 GB",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::Llama321b  => "Meta Llama 3.2 超軽量モデル。プロトタイプ・動作確認やメモリ制約の場面に最適。",
            Self::Llama323b  => "Meta Llama 3.2 軽量モデル。日常的なタスクを軽く試せるバランス型。",
            Self::Llama318b  => "Meta Llama 3.1 標準サイズ。品質・速度・サイズのバランスが良く汎用自然言語処理におすすめ。",
            Self::Mistral7b  => "Mistral AI の英語汎用モデル。指示遵守性が高くコーディング・文章生成に実績あり。",
            Self::Qwen257b   => "Alibaba Qwen 2.5 モデル。多言語対応で数学・コード生成が得意。中国語・英語両方に強い。",
            Self::Gemma31b   => "Google Gemma 3 超小型。モバイル・エッジ等リソース制約環境向け。",
            Self::Gemma34b   => "Google Gemma 3 小型。軽加なローカル実行で虚存アシスタントや要約タスクに最適。",
            Self::Swallow8b  => "[JP] 東工大 Swallow 。Llama 3.1を日本語テキストで追加学習。日本語・英語バランスよく実用的。",
            Self::ElyzaJp8b  => "[JP] ELYZA 日本語特化モデル。Llama 3ベースで日本語の自然な文章生成・対話が得意。",
            Self::Calm322b   => "[JP] CyberAgent calm3 大規模日本語モデル。日本語の論理的思考・長文生成が得意。RAM 16GB以上推奨。",
            Self::Gemma2Jpn2b => "[JP] Google公式 Gemma 2 日本語特化モデル。軽量で日本語の対話・要約に最適。",
            Self::Gemma312b  => "[JP] Google Gemma 3 12B 多言語モデル。日本語・英語ともに高品質。品質と速度のバランスに優れる。",
        }
    }
}

/// Local LLM CLI using llama.cpp bindings (GGUF format).
///
/// Examples:
///   llm-llama --list-models
///   llm-llama --preset llama32-1b --prompt "What is Rust?"
///   llm-llama --model ./model.gguf --prompt "Hello"
#[derive(Parser, Debug)]
#[command(name = "llm-llama", about = "Local LLM CLI using llama.cpp bindings (GGUF format)")]
struct Args {
    /// List available preset models and exit
    #[arg(long)]
    list_models: bool,

    /// Preset model to auto-download from HuggingFace Hub (mutually exclusive with --model)
    #[arg(long, conflicts_with = "model")]
    preset: Option<Preset>,

    /// Path to a local GGUF model file (mutually exclusive with --preset)
    #[arg(long, conflicts_with = "preset")]
    model: Option<PathBuf>,

    /// Prompt to send to the model
    #[arg(short, long)]
    prompt: Option<String>,

    /// Maximum number of tokens to generate
    #[arg(short = 'n', long, default_value_t = 256)]
    max_tokens: u32,

    /// Context window size in tokens
    #[arg(long, default_value_t = 2048)]
    ctx_size: u32,

    /// Sampling temperature (0.0 = deterministic greedy)
    #[arg(short, long, default_value_t = 0.8)]
    temperature: f32,

    /// Random seed for sampling
    #[arg(long, default_value_t = 42)]
    seed: u32,

    /// System prompt for chat session (optional)
    #[arg(long)]
    system: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // ── --list-models ───────────────────────────────────────────────────────
    if args.list_models {
        println!("{:<14} {:<9} {}", "PRESET", "SIZE", "DESCRIPTION");
        println!("{}", "-".repeat(80));
        let mut last_group = "";
        for preset in Preset::value_variants() {
            let name = preset.to_possible_value().unwrap().get_name().to_owned();
            let group = if name.starts_with("swallow") || name.starts_with("elyza") || name.starts_with("calm") || name.starts_with("gemma2-jpn") || name.starts_with("gemma3-12b") {
                "jp"
            } else {
                "en"
            };
            if group != last_group {
                if group == "jp" {
                    println!("  --- 日本語特化 ---");
                }
                last_group = group;
            }
            println!("{:<14} {:<9} {}", name, preset.size(), preset.description());
        }
        return Ok(());
    }

    // ── Resolve model path ──────────────────────────────────────────────────
    let model_path: PathBuf = match (&args.preset, &args.model) {
        (Some(preset), None) => {
            let name = preset.to_possible_value().unwrap().get_name().to_owned();
            eprintln!("[llm-llama] Downloading preset '{name}' from {}", preset.repo_id());
            eprintln!("[llm-llama] File: {}  (cache: ~/.cache/huggingface/hub)", preset.filename());
            let api = Api::new().context("Failed to initialize HuggingFace API")?;
            let repo = api.repo(Repo::new(preset.repo_id().to_owned(), RepoType::Model));
            repo.get(preset.filename())
                .with_context(|| format!("Failed to download {}", preset.filename()))?
        }
        (None, Some(path)) => path.clone(),
        (None, None) => {
            let preset = select_preset_interactive()?;
            let name = preset.to_possible_value().unwrap().get_name().to_owned();
            eprintln!("[llm-llama] Downloading preset '{name}' from {}", preset.repo_id());
            eprintln!("[llm-llama] File: {}  (cache: ~/.cache/huggingface/hub)", preset.filename());
            let api = Api::new().context("Failed to initialize HuggingFace API")?;
            let repo = api.repo(Repo::new(preset.repo_id().to_owned(), RepoType::Model));
            repo.get(preset.filename())
                .with_context(|| format!("Failed to download {}", preset.filename()))?
        }
        (Some(_), Some(_)) => bail!("--model and --preset are mutually exclusive"),
    };

    // ── Init backend + load model ───────────────────────────────────────────
    eprintln!("[llm-llama] Initializing llama.cpp backend...");
    let mut backend = LlamaBackend::init().context("Failed to initialize llama backend")?;
    backend.void_logs();

    eprintln!("[llm-llama] Loading model: {}", model_path.display());
    let model_params = LlamaModelParams::default();
    let model = LlamaModel::load_from_file(&backend, &model_path, &model_params)
        .with_context(|| format!("Failed to load model from {}", model_path.display()))?;

    // --prompt なし → チャットセッションモード
    let prompt = match args.prompt {
        None => return chat_session(&model, &backend, &args),
        Some(p) => p,
    };

    let ctx_size = NonZeroU32::new(args.ctx_size).context("--ctx-size must be > 0")?;
    let ctx_params = LlamaContextParams::default().with_n_ctx(Some(ctx_size));
    let mut ctx = model
        .new_context(&backend, ctx_params)
        .context("Failed to create inference context")?;

    // ── Tokenize + generate ─────────────────────────────────────────────────
    let tokens = model
        .str_to_token(&prompt, AddBos::Always)
        .context("Failed to tokenize prompt")?;
    let n_prompt = tokens.len();
    eprintln!("[llm-llama] Prompt: {} tokens — generating up to {} more", n_prompt, args.max_tokens);
    eprintln!("---");

    print!("{prompt}");
    io::stdout().flush()?;

    let mut batch = LlamaBatch::new(n_prompt.max(512), 1);
    for (i, &token) in tokens.iter().enumerate() {
        let is_last = i == n_prompt - 1;
        batch.add(token, i as i32, &[0], is_last)?;
    }
    ctx.decode(&mut batch).context("Failed to decode prompt")?;

    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::temp(args.temperature),
        LlamaSampler::dist(args.seed),
    ]);

    let mut n_cur = n_prompt;
    for _ in 0..args.max_tokens {
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        sampler.accept(token);

        if model.is_eog_token(token) {
            break;
        }

        #[allow(deprecated)]
        let piece = model
            .token_to_str(token, Special::Tokenize)
            .context("Failed to convert token to string")?;
        print!("{piece}");
        io::stdout().flush()?;

        batch.clear();
        batch.add(token, n_cur as i32, &[0], true)?;
        n_cur += 1;
        ctx.decode(&mut batch).context("Failed to decode token")?;
    }

    println!();
    Ok(())
}

/// Show a numbered preset menu on stderr and return the user's choice.
fn select_preset_interactive() -> Result<Preset> {
    let variants = Preset::value_variants();
    eprintln!("\nAvailable models:");
    eprintln!("{:<4} {:<14} {:<9} {}", "No.", "PRESET", "SIZE", "DESCRIPTION");
    eprintln!("{}", "-".repeat(70));
    let mut last_group = "";
    for (i, preset) in variants.iter().enumerate() {
        let name = preset.to_possible_value().unwrap().get_name().to_owned();
        let group = if name.starts_with("swallow") || name.starts_with("elyza") || name.starts_with("calm") || name.starts_with("gemma2-jpn") || name.starts_with("gemma3-12b") {
            "jp"
        } else {
            "en"
        };
        if group != last_group {
            if group == "jp" {
                eprintln!("  --- 日本語特化 ---");
            }
            last_group = group;
        }
        eprintln!("[{:>2}] {:<14} {:<9} {}", i + 1, name, preset.size(), preset.description());
    }
    eprint!("\nSelect model [1-{}]: ", variants.len());
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input).context("Failed to read from stdin")?;
    let n: usize = input.trim().parse().context("Please enter a number")?;
    if n < 1 || n > variants.len() {
        bail!("Number {} is out of range (1-{})", n, variants.len());
    }
    Ok(variants[n - 1].clone())
}

// ────────────────────────────────────────────────────────────────

/// テキストから URL を抽出する
fn extract_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut search = text;
    loop {
        let pos = match search.find("https://").or_else(|| search.find("http://")) {
            Some(p) => p,
            None => break,
        };
        let from = &search[pos..];
        let end = from
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | ']' | '>' | ','))
            .unwrap_or(from.len());
        let url = from[..end].trim_end_matches('.').to_owned();
        if !url.is_empty() {
            urls.push(url);
        }
        search = if end == 0 { &from[1..] } else { &from[end..] };
    }
    urls.dedup();
    urls
}

/// URL を取得してテキストコンテンツを返す
fn fetch_url_content(url: &str) -> anyhow::Result<String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(15))
        .build();
    let body = agent
        .get(url)
        .set(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36",
        )
        .call()?
        .into_string()?;

    let doc = Html::parse_document(&body);

    // p / heading / li / td 等のコンテンツ要素からテキストを収集
    let sel = Selector::parse(
        "p, h1, h2, h3, h4, h5, h6, li, td, th, blockquote, article",
    )
    .unwrap();

    let mut lines: Vec<String> = doc
        .select(&sel)
        .map(|el| el.text().collect::<Vec<_>>().join(" "))
        .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|t| t.len() > 3)
        .collect();
    lines.dedup();

    let text = lines.join("\n");
    Ok(if text.len() > 8000 {
        format!("{}…[truncated]", &text[..8000])
    } else {
        text
    })
}

// ────────────────────────────────────────────────────────────────
/// Multi-turn chat session.
/// メッセージ履歴を累積しながら KV キャッシュを再利用して高速な対話に対応する。
/// モデルに埋め込まれたチャットテンプレートを自動適用する。
fn chat_session(model: &LlamaModel, backend: &LlamaBackend, args: &Args) -> Result<()> {
    let ctx_size = NonZeroU32::new(args.ctx_size).context("--ctx-size must be > 0")?;
    let ctx_params = LlamaContextParams::default().with_n_ctx(Some(ctx_size));
    let mut ctx = model
        .new_context(backend, ctx_params)
        .context("Failed to create inference context")?;

    // モデルバインドのチャットテンプレートを取得
    let tmpl = model
        .chat_template(None)
        .context("Failed to retrieve chat template from model")?;

    let mut messages: Vec<LlamaChatMessage> = Vec::new();
    // 保存用ミラー（serde 対応）
    let mut saved_msgs: Vec<SavedMessage> = Vec::new();

    // システムプロンプトを設定
    if let Some(sys) = &args.system {
        messages.push(
            LlamaChatMessage::new("system".to_owned(), sys.clone())
                .context("Failed to create system message")?,
        );
        saved_msgs.push(SavedMessage { role: "system".to_owned(), content: sys.clone() });
    }

    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::temp(args.temperature),
        LlamaSampler::dist(args.seed),
    ]);
    let mut n_past: usize = 0;

    // rustyline エディタ（↑↓ 入力履歴、行編集）
    let mut rl = DefaultEditor::new().context("Failed to initialize readline")?;

    // 自動保存用セッション名（最初のユーザー入力から決定 or ロード時に設定）
    let mut auto_session_name: Option<String> = None;

    eprintln!("\n[Chat session started]");
    if args.system.is_some() {
        eprintln!("[System prompt: set]");
    }
    eprintln!("Chat commands:");
    eprintln!("  /sessions   過去の会話一覧を表示し、↑↓で選んで再開");
    eprintln!("  /review     過去の会話を↑↓で選んで内容を見直す");
    eprintln!("  /delete     過去の会話を↑↓で選んで削除");
    eprintln!("  /new        現在の会話を破棄して新しいチャットを開始");
    eprintln!("  /exit       終了（Ctrl-D も可）");
    eprintln!();

    loop {
        let prompt_str = format!("{BOLD}{GREEN}You:{RESET} ", BOLD=BOLD, GREEN=GREEN, RESET=RESET);
        let readline = rl.readline(&prompt_str);
        let user_msg = match readline {
            Ok(line) => {
                let trimmed = line.trim_end_matches('\n').trim_end_matches('\r').to_owned();
                if !trimmed.is_empty() {
                    let _ = rl.add_history_entry(&trimmed);
                }
                trimmed
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => break,
            Err(e) => return Err(e).context("Readline error"),
        };

        if user_msg.is_empty() {
            continue;
        }

        // ── スラッシュコマンド処理 ────────────────────────────────────────
        if user_msg == "/exit" {
            break;
        }
        if user_msg == "/new" {
            eprintln!("[New session started — previous context cleared]");
            messages.clear();
            saved_msgs.clear();
            n_past = 0;
            auto_session_name = None;
            // ctx を作り直して KV キャッシュもリセット
            ctx = model
                .new_context(backend, LlamaContextParams::default().with_n_ctx(Some(ctx_size)))
                .context("Failed to recreate context")?;
            if let Some(sys) = &args.system {
                messages.push(
                    LlamaChatMessage::new("system".to_owned(), sys.clone())
                        .context("Failed to create system message")?,
                );
                saved_msgs.push(SavedMessage { role: "system".to_owned(), content: sys.clone() });
            }
            continue;
        }
        if user_msg == "/review" {
            match select_session_for_review() {
                Err(e) => { eprintln!("[Error: {e}]"); continue; }
                Ok(None) => { eprintln!("[Cancelled]"); continue; }
                Ok(Some(name)) => {
                    match load_session(&name) {
                        Err(e) => eprintln!("[Load failed: {e}]"),
                        Ok(sess) => print_session_review(&sess),
                    }
                    continue;
                }
            }
        }
        if user_msg == "/delete" {
            match select_session_for_delete() {
                Err(e) => { eprintln!("[Error: {e}]"); continue; }
                Ok(None) => { eprintln!("[Cancelled]"); continue; }
                Ok(Some(name)) => {
                    match delete_session(&name) {
                        Ok(()) => eprintln!("[Session '{name}' deleted]"),
                        Err(e) => eprintln!("[Delete failed: {e}]"),
                    }
                    continue;
                }
            }
        }
        if user_msg == "/sessions" {
            match select_session_interactive() {
                Err(e) => { eprintln!("[Error: {e}]"); continue; }
                Ok(None) => { eprintln!("[Cancelled]"); continue; }
                Ok(Some(name)) => {
                    match load_session(&name) {
                        Err(e) => { eprintln!("[Load failed: {e}]"); continue; }
                        Ok(sess) => {
                            eprintln!("[Loading session '{}' ({} messages)]", sess.name, sess.messages.len());
                            messages.clear();
                            saved_msgs.clear();
                            n_past = 0;
                            ctx = model
                                .new_context(backend, LlamaContextParams::default().with_n_ctx(Some(ctx_size)))
                                .context("Failed to recreate context")?;
                            for m in &sess.messages {
                                messages.push(
                                    LlamaChatMessage::new(m.role.clone(), m.content.clone())
                                        .context("Failed to recreate message")?,
                                );
                                saved_msgs.push(m.clone());
                                if m.role == "user" { let _ = rl.add_history_entry(&m.content); }
                            }
                            if !messages.is_empty() {
                                match model.apply_chat_template(&tmpl, &messages, false) {
                                    Ok(text) => {
                                        if let Ok(toks) = model.str_to_token(&text, AddBos::Always) {
                                            let mut batch = LlamaBatch::new(toks.len().max(512), 1);
                                            for (i, &tok) in toks.iter().enumerate() {
                                                let _ = batch.add(tok, i as i32, &[0], i == toks.len() - 1);
                                            }
                                            if ctx.decode(&mut batch).is_ok() {
                                                n_past = toks.len();
                                            }
                                        }
                                    }
                                    Err(_) => {}
                                }
                                eprintln!("[Context restored — continue the conversation]");
                            }
                            auto_session_name = Some(name);
                            continue;
                        }
                    }
                }
            }
        }

        // ── URL フェッチ ──────────────────────────────────────────────────
        let urls = extract_urls(&user_msg);
        let augmented_msg = if urls.is_empty() {
            user_msg.clone()
        } else {
            let mut ctx_block = String::new();
            for url in &urls {
                eprint!("\n[fetching {}] ", url);
                io::stderr().flush()?;
                match fetch_url_content(url) {
                    Ok(content) if !content.trim().is_empty() => {
                        eprintln!("({} chars)", content.len());
                        ctx_block.push_str(&format!(
                            "[Retrieved content from {}]\n{}\n---\n",
                            url, content
                        ));
                    }
                    Ok(_) => {
                        eprintln!("no text content (page may require JavaScript)");
                        ctx_block.push_str(&format!(
                            "[Could not retrieve text content from {} (page may require JavaScript)]\n---\n",
                            url
                        ));
                    }
                    Err(e) => {
                        eprintln!("failed: {}", e);
                        ctx_block.push_str(&format!(
                            "[Fetch failed for {}: {}]\n---\n",
                            url, e
                        ));
                    }
                }
            }
            eprintln!();
            if ctx_block.is_empty() { user_msg.clone() } else { format!("{}{}", ctx_block, user_msg) }
        };

        // ── メッセージ履歴に追加 ──────────────────────────────────────────
        messages.push(
            LlamaChatMessage::new("user".to_owned(), augmented_msg.clone())
                .context("Failed to create user message")?,
        );
        saved_msgs.push(SavedMessage { role: "user".to_owned(), content: user_msg.clone() });

        // 初回メッセージでセッション名を自動生成（未設定の場合のみ）
        if auto_session_name.is_none() {
            let slug: String = user_msg
                .chars()
                .take(40)
                .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
                .collect::<String>()
                .trim_matches('_')
                .to_owned();
            let ts = chrono_now().replace([':', ' ', '-'], "_");
            auto_session_name = Some(format!("{ts}_{slug}"));
        }

        // 履歴全体にチャットテンプレートを適用
        let formatted = model
            .apply_chat_template(&tmpl, &messages, true)
            .context("Failed to apply chat template")?;

        let full_tokens = model
            .str_to_token(&formatted, AddBos::Always)
            .context("Failed to tokenize")?;

        if full_tokens.len() <= n_past {
            eprintln!("[warning] No new tokens to process; try a shorter conversation");
            messages.pop();
            saved_msgs.pop();
            continue;
        }

        let new_tokens = &full_tokens[n_past..];
        let mut batch = LlamaBatch::new(new_tokens.len().max(512), 1);
        for (i, &token) in new_tokens.iter().enumerate() {
            let pos = (n_past + i) as i32;
            let is_last = i == new_tokens.len() - 1;
            batch.add(token, pos, &[0], is_last)?;
        }
        ctx.decode(&mut batch).context("Failed to decode prompt tokens")?;
        n_past += new_tokens.len();

        eprint!("\n{BOLD}{CYAN}Assistant:{RESET} ", BOLD=BOLD, CYAN=CYAN, RESET=RESET);
        io::stderr().flush()?;

        let mut assistant_text = String::new();
        'generate: for _ in 0..args.max_tokens {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);

            if model.is_eog_token(token) {
                break 'generate;
            }

            #[allow(deprecated)]
            let piece = model
                .token_to_str(token, Special::Tokenize)
                .context("Failed to decode token")?;
            print!("{CYAN}{piece}{RESET}", CYAN=CYAN, RESET=RESET);
            io::stdout().flush()?;
            assistant_text.push_str(&piece);

            batch.clear();
            batch.add(token, n_past as i32, &[0], true)?;
            ctx.decode(&mut batch).context("Failed to decode generated token")?;
            n_past += 1;
        }
        println!("\n");

        messages.push(
            LlamaChatMessage::new("assistant".to_owned(), assistant_text.clone())
                .context("Failed to create assistant message")?,
        );
        saved_msgs.push(SavedMessage { role: "assistant".to_owned(), content: assistant_text });

        // アシスタント応答のたびに自動保存
        if let Some(ref sname) = auto_session_name {
            if let Err(e) = save_session(sname, &saved_msgs) {
                eprintln!("[auto-save failed: {e}]");
            }
        }
    }

    // セッション終了時にも保存
    if let Some(ref sname) = auto_session_name {
        if saved_msgs.iter().any(|m| m.role != "system") {
            match save_session(sname, &saved_msgs) {
                Ok(path) => eprintln!("[Session auto-saved → {}]", path.display()),
                Err(e) => eprintln!("[auto-save failed: {e}]"),
            }
        }
    }

    eprintln!("[Session ended]");
    Ok(())
}

