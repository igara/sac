# sac — Local LLM CLI in Rust

llama.cpp バインディングを使って GGUF モデルをローカルで動かす CLI ツールです。

---

## 必要環境

- Rust 1.94.1+
- macOS / Linux（Windows は未確認）

---

## ビルド

```sh
cargo build --release
```

バイナリは `target/release/llm-llama` に生成されます。

---

## モデル一覧

```sh
llm-llama --list-models
```

プリセットを指定すると HuggingFace Hub から自動ダウンロードします（キャッシュ: `~/.cache/huggingface/hub`）。

| プリセット | サイズ | 説明 |
|---|---|---|
| `llama32-1b` | ~0.7 GB | Meta Llama 3.2 超軽量。プロトタイプ・動作確認に |
| `llama32-3b` | ~1.9 GB | Meta Llama 3.2 軽量。日常タスクのバランス型 |
| `llama31-8b` | ~4.7 GB | Meta Llama 3.1 標準。品質・速度・サイズのバランスが良い|
| `mistral-7b` | ~4.1 GB | Mistral AI 英語汎用。コーディング・文章生成に実績あり |
| `qwen25-7b` | ~4.4 GB | Alibaba Qwen 2.5。多言語・数学・コード生成が得意 |
| `gemma3-1b` | ~0.8 GB | Google Gemma 3 超小型。リソース制約環境向け |
| `gemma3-4b` | ~2.5 GB | Google Gemma 3 小型。軽量ローカル実行向け |
| `swallow-8b` | ~4.9 GB | **[JP]** 東工大 Swallow。日本語+英語バランス型 |
| `elyza-jp-8b` | ~4.9 GB | **[JP]** ELYZA 日本語特化。自然な対話・文章生成が得意 |
| `calm3-22b` | ~13 GB | **[JP]** CyberAgent calm3 大規模日本語。RAM 16GB〜 |
| `gemma2-jpn-2b` | ~1.4 GB | **[JP]** Google公式 Gemma 2 日本語特化。軽量で日本語対話向け |
| `gemma3-12b` | ~7.0 GB | **[JP]** Google Gemma 3 12B 多言語。日本語・英語両方に高品質 |

---

## 使い方

```sh
# オプションなしで起動 → 対話的にモデルとプロンプトを選択
llm-llama

# プリセット指定（初回は自動ダウンロード）
llm-llama --preset elyza-jp-8b --prompt "Rustの特徴を教えて"

# 生成長・温度を調整
llm-llama --preset llama31-8b --prompt "What is Rust?" --max-tokens 512 --temperature 0.7

# ローカルの GGUF ファイルを直接指定
llm-llama --model ./my-model.gguf --prompt "Hello"
```

---

## オプション一覧

| オプション | デフォルト | 説明 |
|---|---|---|
| `--list-models` | — | プリセット一覧を表示して終了 |
| `--preset <name>` | — | プリセット名でモデルを選択（`--model` と排他） |
| `--model <path>` | — | ローカル GGUF ファイルのパス（`--preset` と排他） |
| `--prompt`, `-p` | — | 入力プロンプト（省略時はチャットセッションを起動） |
| `--system` | — | チャットセッション用システムプロンプト |
| `--max-tokens`, `-n` | `256` | 生成する最大トークン数 |
| `--ctx-size` | `2048` | コンテキストウィンドウサイズ |
| `--temperature`, `-t` | `0.8` | サンプリング温度（0.0 = 決定的） |
| `--seed` | `42` | 乱数シード |

---

## チャットセッション

`--prompt` を省略して起動するとチャットモードに入ります。↑↓キーで過去に入力したメッセージを呼び出せます。  
会話は **自動保存** され、`~/.local/share/llm-llama/sessions/` に JSON で蓄積されます。

```sh
# システムプロンプト付きで起動
llm-llama --preset elyza-jp-8b --system "あなたは優秀なアシスタントです"
```

### チャット中のコマンド

| コマンド | 説明 |
|---|---|
| `/sessions` | 過去の会話一覧を表示し、↑↓で選んで再開 |
| `/review` | 過去の会話を↑↓で選んで内容を見直す |
| `/delete` | 過去の会話を↑↓で選んで削除 |
| `/new` | 現在の会話を破棄して新しいチャットを開始 |
| `/exit` | 終了（Ctrl-D も可） |


