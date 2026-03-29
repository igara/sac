# llm-cli-llama - 開発環境設定

## プロジェクト概要
このプロジェクトはRustで書かれたLLM（大規模言語モデル）CLIツールです。Ollamaと連携して、ローカル環境で大規模言語モデルを実行・利用できます。

## 開発環境構成

### 使用ツール
- **Claude Code**: AIコード補完ツール
- **VSCode + Continue**: 開発環境
- **Ollama**: ローカルでのLLM実行環境
- **qwen3-coder:30b**: 利用するLLMモデル

### ディレクトリ構成
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

## 開発手順

### 1. 環境準備
- Ollamaをインストールし、qwen3-coder:30bモデルをダウンロード
- Rust環境をセットアップ
- VSCodeにContinue拡張をインストール

### 2. プロジェクトのビルド
```bash
cd llm-cli-llama
cargo build
```

### 3. 実行
```bash
cd llm-cli-llama
cargo run
```

## ツールの使用方法

### Claude Code
- コードの自動補完や生成に使用
- プロジェクトの理解を深めるために活用

### VSCode + Continue
- コード編集環境
- AIによるコード補完やリファクタリング支援

### Ollama + qwen3-coder:30b
- ローカルでのLLM実行環境
- モデルのダウンロードと起動
  ```bash
  ollama run qwen3-coder:30b
  ```

## 開発ルール
1. Rustのコーディング規則に従う
2. コメントを適切に記述する
3. テストを追加して機能を検証する
4. 変更点をGitで管理する
