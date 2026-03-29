
# llm-cli-llama - 開発・利用ガイド


## プロジェクト概要
Rust製のLLM（大規模言語モデル）CLIツールです。llama.cppバインディングを使い、GGUFモデルをローカルで動かせます。Ollamaやqwen3-coder:30bとも連携可能。


## 必要環境
- Rust 1.94.1+
- macOS / Linux（Windows未確認）


## ディレクトリ構成
```
.
├── .git/
├── .gitignore
├── .tool-versions
├── Cargo.lock
├── Cargo.toml
├── README.md
└── llm-cli-llama/
  ├── Cargo.toml
  └── src/
    └── main.rs
```


## セットアップ・ビルド
1. Rust環境をセットアップ
2. 必要に応じてOllamaをインストールし、qwen3-coder:30bモデルをダウンロード
3. VSCodeにContinue拡張をインストール

### ビルド
```sh
cd llm-cli-llama
cargo build --release
```
バイナリは `target/release/llm-llama` に生成されます。

### 実行
```sh
cd llm-cli-llama
cargo run
```


## モデル一覧

```sh
llm-llama --list-models
```
プリセットを指定すると HuggingFace Hub から自動ダウンロードします（キャッシュ: `~/.cache/huggingface/hub`）。

| プリセット | サイズ | 説明 |
|---|---|---|
| `llama32-1b` | ~0.7 GB | Meta Llama 3.2 超軽量モデル。プロトタイプ・動作確認やメモリ制約の場面に最適。 |
| `llama32-3b` | ~1.9 GB | Meta Llama 3.2 軽量モデル。日常的なタスクを軽く試せるバランス型。 |
| `llama31-8b` | ~4.7 GB | Meta Llama 3.1 標準サイズ。品質・速度・サイズのバランスが良く汎用自然言語処理におすすめ。 |
| `mistral-7b` | ~4.1 GB | Mistral AI の英語汎用モデル。指示遵守性が高くコーディング・文章生成に実績あり。 |
| `qwen25-7b` | ~4.4 GB | Alibaba Qwen 2.5 モデル。多言語対応で数学・コード生成が得意。中国語・英語両方に強い。 |
| `gemma3-1b` | ~0.8 GB | Google Gemma 3 超小型。モバイル・エッジ等リソース制約環境向け。 |
| `gemma3-4b` | ~2.5 GB | Google Gemma 3 小型。軽加なローカル実行で虚存アシスタントや要約タスクに最適。 |
| `swallow-8b` | ~4.9 GB | [JP] 東工大 Swallow 。Llama 3.1を日本語テキストで追加学習。日本語・英語バランスよく実用的。 |
| `elyza-jp-8b` | ~4.9 GB | [JP] ELYZA 日本語特化モデル。Llama 3ベースで日本語の自然な文章生成・対話が得意。 |
| `calm3-22b` | ~13 GB | [JP] CyberAgent calm3 大規模日本語モデル。日本語の論理的思考・長文生成が得意。RAM 16GB以上推奨。 |
| `gemma2-jpn-2b` | ~1.4 GB | [JP] Google公式 Gemma 2 日本語特化モデル。軽量で日本語の対話・要約に最適。 |
| `gemma3-12b` | ~7.0 GB | [JP] Google Gemma 3 12B 多言語モデル。日本語・英語ともに高品質。品質と速度のバランスに優れる。 |


## 使い方・コマンド例

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

### 主なオプション

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

### トークン数・文字数の制限
デフォルトで「生成最大トークン数256」「コンテキストウィンドウ2048トークン」に制限。必要に応じて `--max-tokens` や `--ctx-size` で調整。

### チャットセッション
`--prompt` を省略して起動するとチャットモードに。会話は自動保存され、`~/.local/share/llm-llama/sessions/` にJSONで蓄積。

#### チャット中のコマンド
| コマンド | 説明 |
|---|---|
| `/sessions` | 過去の会話一覧を表示し、↑↓で選んで再開 |
| `/review` | 過去の会話を↑↓で選んで内容を見直す |
| `/delete` | 過去の会話を↑↓で選んで削除 |
| `/new` | 現在の会話を破棄して新しいチャットを開始 |
| `/exit` | 終了（Ctrl-D も可） |

---

## 開発ルール
1. Rustのコーディング規則に従う
2. コメントを適切に記述する
3. テストを追加して機能を検証する
4. 変更点をGitで管理する

## 開発・利用Tips
- Claude Code: コード自動補完や生成に活用
- VSCode + Continue: AIによるコード補完やリファクタリング支援
- Ollama + qwen3-coder:30b: ローカルLLM実行環境。`ollama run qwen3-coder:30b` で起動
