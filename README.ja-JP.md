# VaultOne

> **VaultOne はあなたの AI データを保存しません。あなたが既に所有しているデータを理解し、管理することを支援します。**

AI CLI のトークン使用量とコストを可視化する、ローカルファーストのデスクトップダッシュボード。ツールが既に書き出すセッションログ（**Claude Code、Codex、Gemini CLI、OpenCode**）を直接読み取り、必要に応じて自分が管理する GitHub リポジトリ経由で複数端末間同期できます。

[![Version](https://img.shields.io/github/v/release/Buktal/VaultOne?color=blue&label=version)](https://github.com/Buktal/VaultOne/releases)
[![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](https://github.com/Buktal/VaultOne/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](./LICENSE)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)

[English](./README.md) | [简体中文](./README.zh-CN.md) | **日本語** | [更新履歴](./CHANGELOG.ja-JP.md)

<img src="./docs/images/ad-ja.png" alt="VaultOne ダッシュボード">

---

## なぜ VaultOne？

AI CLI は実行のたびにセッションログをディスクに書き出します。VaultOne はそのログを鮮明な使用量の姿——**トークン・コスト・キャッシュ効率・トレンド**——に変えます。プロキシも API キーも、データをどこかへ送る必要もありません。

製品全体を形作る 2 つのスタンス:

- **ローカルファースト。** ダッシュボードはネットワークゼロで動きます。自分のログを読むだけで十分です。
- **読み取り専用。** VaultOne はセッションログを*読む*だけです。決して変更せず、ツールの挙動にも一切干渉しません。ツールは以前と全く同じように動き続けます。

複数端末間同期は存在しますが、純粋に**オプトイン**の上乗せ層であり、アプリを使うための前提では決してありません。

## ハイライト

- **4 つの AI CLI のログを読む** —— Claude Code、Codex、Gemini CLI、OpenCode。それぞれネイティブ形式でディスクから直接解析。プロキシ不要、API キー不要、ネットワーク不要。
- **実際の課金に合うトークン口径** —— 4 バケット消費（input / output / cache creation / cache read）、キャッシュヒット率、コストを収集時に取得して固定。ソースごとの差異（例：Codex のキャッシュ込み input）は正規化され、一貫した 1 つのモデルに。
- **自分の GitHub リポジトリで複数端末同期** —— 使用量データはプレーンテキストとして、端末と日付で分割され、あなたが管理するリポジトリへ。間に第三者サービスを挟みません。さらに任意のビューを単一端末に絞り込み可能。
- **端末別ファイル中継（Library）** —— ファイル / ディレクトリをアプリにドラッグして同期リポジトリ経由で中継（各端末は自身のサブツリーに書き込み、衝突なし）。アプリ内プレビュー、任意パスへエクスポート。アップロードが唯一の自動方向で、AI ツール自身の設定ディレクトリには一切書き込みません。
- **ライトウェイトモード** —— 画面端のミニバーで今日の合計を常時表示、あるいはダッシュボードを再利用したフローティングカードに展開。full ⇄ expanded ⇄ tucked をどの形からも切り替え、各形状は自身の配置を記憶。
- **マルチスキンテーマ** —— 5 つのアクセント＋チャート配色（Neutral / Sage / Azure / Crimson / Mauve）。内容に触れずアプリ全体をリカラー。
- **トレイ常駐のバックグラウンド収集** —— 増分スキャナが背後でダッシュボードを最新に保ちます。ウィンドウ不要。
- **自動更新＆3 言語** —— GitHub Releases から署名付きアップデートを直接インストール。UI は English / 简体中文 / 日本語。

## スクリーンショット

| | ライト | ダーク |
| --- | --- | --- |
| **ダッシュボード** | <img src="./docs/images/light-usage.png" alt="ダッシュボード（ライト）" width="320"> | <img src="./docs/images/dark-usage.png" alt="ダッシュボード（ダーク）" width="320"> |
| **消費** | <img src="./docs/images/light-consumption.png" alt="消費（ライト）" width="320"> | <img src="./docs/images/dark-consumption.png" alt="消費（ダーク）" width="320"> |
| **グランスモード** | <img src="./docs/images/light-floating-card.png" alt="グランスモード（ライト）" width="320"> | <img src="./docs/images/dark-floating-card.png" alt="グランスモード（ダーク）" width="320"> |

## ダウンロード

**[Releases](https://github.com/Buktal/VaultOne/releases)** ページから、お使いの OS 向けインストーラを入手してください。

| OS | インストーラ |
| --- | --- |
| **Windows** | `.msi` または `.exe`（NSIS）セットアップ |
| **macOS** | `.dmg`（Apple Silicon / arm64） |
| **Linux** | `.deb`、`.AppImage`（入手可能なら `.rpm`） |

**初回起動:** VaultOne を起動すると、ローカルの AI CLI セッションログをスキャンし、ダッシュボードが埋まります。アカウント不要、サインイン不要、ネットワーク不要。複数台で使用量を見るには、**設定**で同期を有効にし、自分が管理する GitHub リポジトリを指定します。

> **macOS の注意:** 現在のビルドは未署名です。初回起動時にアプリを右クリック → **開く**、もしくは隔離属性を除去してください:
> ```bash
> xattr -dr com.apple.quarantine /Applications/VaultOne.app
> ```

## 機能

### ダッシュボード

- **4 バケットのトークン消費** —— input、output、cache creation、cache read。
- **キャッシュヒット率** —— `cache_read / (input + cache_creation + cache_read)`。上流の使用量集計と整合。
- **リクエスト数とコスト** —— 総リクエスト数と総コスト（USD）、収集時に固定。
- **使用量トレンド** —— マルチラインのトークン対コストチャート、指標ごとに 1 系列。
- **コールごとのリクエストログ** —— ソース、モデル、トークン内訳、コスト、ターン所要時間、`stop_reason` / `service_tier` チップ。
- **ターンごとのビュー** —— ターン全体のコストと実所要時間、単一コールの計時とは別。

### 収集

- **読み取り専用ソース** —— CLI が既に書き出すセッションログを解析。決して変更しません。
- **増分スキャン** —— カーソルベースのスキャナが変化分だけを拾います。
- **トレイ常駐のバックグラウンドスケジューラ** —— ウィンドウを開いたままにせず、タイマーで収集します。
- **プラグイン可能なプロバイダ** —— 現在 Claude Code、Codex、Gemini CLI、OpenCode。それぞれネイティブのログ形式（JSONL、JSON、SQLite）から解析し、トークン口径を 1 つの 4 バケットモデルに正規化。

### 同期（任意）

- **スタンドアローンモード** —— 完全なダッシュボード、ネットワークゼロ。
- **同期モード** —— 自分が管理する GitHub リポジトリ経由で、端末間の使用量を整合。
- **デバイススコープ** —— ダッシュボード、グランスカード、格納バーを単一デバイスに絞り込み。ピアをローカルから忘却でき、古いピアは自動消去。
- **システムプロキシ対応** —— push/fetch が OS プロキシ（Clash/Mihomo、企業ゲートウェイ）に追従し、プロキシ環境下でも同期がそのまま動作。
- **プレーンテキストのアーティファクト** —— 端末と日付で分割（`data/<device>/usage-YYYY-MM-DD.jsonl`）。diff が読みやすく査読可能。
- **衝突なしの自動復旧** —— 各端末は自身の `data/<device>/` サブツリーに書き込むため、同時 push は衝突しません。push 競合に負けても、次回同期がローカルコミットをリモート先頭へリベースして自己修復します。収集された各行は同期アーティファクトにも確実に到達し、万が一行が漏れても次回収集で埋め戻されるため、手動の git 操作やスタック状態なしに端末間で収束します。

### 中継ストレージ（Library）

- **ドラッグでアップロード** —— ファイル / ディレクトリをドロップすると、同期リポジトリの該当端末サブツリーへアップロード（＝ push）。ネストしたディレクトリは任意の深さで動作。
- **アプリ内プレビュー** —— 画像は幅にフィットし ctrl+ホイールでズーム、それ以外はサンドボックス iframe で描画。
- **手動エクスポート** —— ファイルダイアログで任意パスへ保存。VaultOne は対象パスを知らず、AI ツールの設定ディレクトリには書き込みません。
- **安全な上書き** —— 同名同種は上書き（git 履歴が安全網）、同名異種は拒否。
- **端末別・衝突なし** —— 各端末は自身のサブツリーを保持。ピアの忘却ではそのファイルを `from-<peer>/` へ移行するか削除するかを選択。

### コストと価格

- **編集可能なモデルごとの価格** —— シード価格を上書き。VaultOne はあなたの数値を使います。
- **再請求（Rebill）** —— 収集時に価格が無かったレコードを遡って計上。既存履歴を再計算しません。

### 体験

- **ライトウェイトモード** —— エッジ格納のミニバー ＋ 展開可能なフローティングカード、各形状が自身の配置を記憶。
- **マルチスキンテーマ** —— 5 つの配色、デフォルトは Neutral（グレースケール）。
- **自動更新** —— GitHub Releases から署名付きインストーラを直接取得、設定で手動チェックも可。
- **ライト / ダークテーマ、3 言語、デフォルトでプライベート** —— 同期を有効にしない限り、使用量データはあなたの端末に留まります。

## 仕組み

```
  AI CLI セッションログ
 （Claude Code · Codex · Gemini CLI · OpenCode）
          │（読み取り専用）
          ▼
       収集 ──────▶ ローカルストア ──────▶ ダッシュボード
          │
          │（任意 · 同期モード）
          ▼
   アーティファクト（プレーンテキスト、端末 + 日付ごと）
          │
    あなたの GitHub リポジトリで push / pull
          │
          ▼
     他の端末
```

[Tauri 2](https://tauri.app/) アプリ: Rust バックエンドが収集・ローカルストア・オプションの Git リポジトリ同期を担い、React フロントエンドが生成された型安全な IPC バインディング経由でダッシュボードを描画します。収集器はプラグイン可能なプロバイダモデル（Claude Code、Codex、Gemini CLI、OpenCode）、ローカルストアはダッシュボードの唯一の読み取り元、同期はそのストアを端末と日付で分割したプレーンテキストのアーティファクトへ投影するオプトインの層です。

## ソースからビルド

**前提条件:** [Node.js](https://nodejs.org/) LTS + [Yarn](https://yarnpkg.com/)、および [Rust](https://www.rust-lang.org/) stable（OS ごとの [Tauri の前提条件](https://tauri.app/start/prerequisites/)を参照）。

```bash
yarn install     # 依存をインストール
yarn dev         # デスクトップアプリを開発モードで実行
yarn dist        # リリース版バイナリをビルド
yarn check       # 静的チェック（Biome + tsc + Rust fmt/clippy）—— CI と同構
yarn test        # テストスイートを実行
```

**技術スタック:** [Tauri 2](https://tauri.app/)（Rust）· [React 19](https://react.dev/) · [TypeScript](https://www.typescriptlang.org/) · [Vite](https://vite.dev/) · [Tailwind CSS v4](https://tailwindcss.com/) · [shadcn/ui](https://ui.shadcn.com/) · [Redux Toolkit](https://redux-toolkit.js.org/) · [Recharts](https://recharts.org/)

## コントリビュート

Issue と提案を歓迎します。PR を出す前に `yarn check` と `yarn test` を実行し、CI ゲートをローカルで通してください。大きな機能は、まず Issue を開いて方針を議合してください。

## ライセンス

[MIT](./LICENSE) © VaultOne Contributors
