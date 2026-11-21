<p align="center">
  <img src="docs/assets/brand/logo.png" width="150" alt="AgentVault logo">
</p>

<h1 align="center">AgentVault</h1>

<p align="center">
  <strong>AI エージェントの長期クレデンシャルを、使い捨てチケットに変える。</strong><br>
  長期シークレットは OS のキーストアに残し、エージェントには期限付き・失効可能・既定で 1 回限りのチケットを渡します。
</p>

<p align="center">
  <a href="https://github.com/Seramys/agentvault/actions/workflows/ci.yml"><img src="https://github.com/Seramys/agentvault/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-6D5BD0.svg" alt="License"></a>
  <img src="https://img.shields.io/badge/rust-stable-CE422B.svg" alt="Rust stable">
  <img src="https://img.shields.io/badge/status-v0.1-0F9F6E.svg" alt="v0.1">
</p>

<p align="center">
  <a href="README.md">简体中文</a> · <a href="README_EN.md">English</a> · <b>日本語</b> · <a href="README_KO.md">한국어</a>
</p>

<p align="center">
  <a href="#30-秒で理解">30 秒</a> ·
  <a href="#クイックスタート">クイックスタート</a> ·
  <a href="#図解ステップガイド">ガイド</a> ·
  <a href="#コマンド一覧">コマンド</a> ·
  <a href="#セキュリティモデル">セキュリティ</a> ·
  <a href="#実テストの証拠">証拠</a>
</p>

> **v0.1 の範囲**：レイヤー 0（シークレット保管）、1（短期チケット + ローカル broker）、4（監査 + ハニーポット）を実装済みです。サンドボックス（L2）と egress 制御/DLP（L3）は明示的に対象外です。Web コンソールは延期し、このリポジトリは CLI のみを公開します。
>
> **ひとことで**：「この token を `mcp.json` に書くべきか」を悩まない。Vault に保管し、エージェントにはチケットと交換させる。

---

## 目次

<details>
<summary>展開</summary>

- [解決する問題](#解決する問題)
- [30 秒で理解](#30-秒で理解)
- [これは何で、何ではないか](#これは何で何ではないか)
- [対応エージェント](#対応エージェント)
- [5 つのレイヤー](#5-つのレイヤー)
- [インストール](#インストール)
- [クイックスタート](#クイックスタート)
- [図解ステップガイド](#図解ステップガイド)
  - [ステップ 0：全体像](#ステップ-0全体像)
  - [ステップ 1：インストールと確認](#ステップ-1インストールと確認)
  - [ステップ 2：エージェント設定の検出](#ステップ-2エージェント設定の検出)
  - [ステップ 3：dry-run で何も変更しない](#ステップ-3dry-run-で何も変更しない)
  - [ステップ 4：init を実行する](#ステップ-4init-を実行する)
  - [ステップ 5：最初のチケットを発行する](#ステップ-5最初のチケットを発行する)
  - [ステップ 6：1 回で使用、再生は拒否](#ステップ-61-回で使用再生は拒否)
  - [ステップ 7：タスクチケットと子チケット](#ステップ-7タスクチケットと子チケット)
  - [ステップ 8：実プロセスをラップする](#ステップ-8実プロセスをラップする)
  - [ステップ 9：アップグレード後に launcher を更新](#ステップ-9アップグレード後に-launcher-を更新)
  - [ステップ 10：退職・端末紛失・ハニーポット](#ステップ-10退職端末紛失ハニーポット)
  - [ステップ 11：実テストを実行](#ステップ-11実テストを実行)
- [設定検出と env 移行](#設定検出と-env-移行)
- [コマンド一覧](#コマンド一覧)
- [初回実行と AGENTVAULT_HOME](#初回実行と-agentvault_home)
- [実テストの証拠](#実テストの証拠)
- [セキュリティモデル](#セキュリティモデル)
- [既知の限界](#既知の限界)
- [FAQ](#faq)
- [リポジトリ構成](#リポジトリ構成)
- [ロードマップ](#ロードマップ)
- [コントリビュートとセキュリティ](#コントリビュートとセキュリティ)
- [ライセンス](#ライセンス)

</details>

---

## 解決する問題

Claude Code、Cursor、Codex、Qoder などの MCP クライアントをデータベース、GitHub、ブラウザ、社内 API に接続するとき、よくある方法は設定にシークレットを直接書くことです。

```json
{
  "mcpServers": {
    "postgres": {
      "command": "npx",
      "args": ["@modelcontextprotocol/server-postgres"],
      "env": {
        "DATABASE_URL": "postgres://app:real-password@prod:5432/app",
        "API_KEY": "sk-live-..."
      }
    }
  }
}
```

そこから 3 つの現実的な問題が生まれます。

1. **設定ファイルが平文のシークレット置き場になる**：コピー、スクリーンショット、git commit でシークレットも一緒に移動します。
2. **権限に時間の境界がない**：エージェントが値を得たら永久に使えます。「15 分だけ読み取り」は人の自制に依存します。
3. **インシデント時に答えられない**：誰が、いつ、どのキーを使ったのか。監査も失効もワンタイム性もありません。

AgentVault は流れを変えます。

```text
長期シークレット  ──►  OS キーストア  ──►  短期チケット（既定 1 回）  ──►  エージェント
       ▲                                      │
       └──────── 監査 / 失効 / ローテーション ◄─┘
```

設定に平文は残りません。エージェントは `av1...` チケットを受け取ります。期限切れ、使用済み、失効、グローバル kill switch のいずれかで resolve は拒否されます。

---

## 30 秒で理解

<p align="center">
  <img src="docs/assets/diagrams/architecture.svg" alt="AgentVault architecture" width="100%">
</p>

3 つの役割：

| 役割 | 行うこと | 行わないこと |
|---|---|---|
| **Vault** | 長期シークレットを OS キーストアに保管し、チケットを署名・検証 | 長期シークレットをエージェントに渡す |
| **Broker** | `127.0.0.1` のみで待ち受け、issue/resolve/revoke/rotate を処理 | ネットワーク公開、リモートサービス化 |
| **Agent wrapper** | `agentvault run` で MCP server を起動し、チケットまたは短期値を注入 | ラップ対象の MCP プロトコルを変更 |

最短の体験：

```bash
# 1. 長期シークレットを保存（設定には入らない）
agentvault secret add demo/token --value 'sk-test-demo-123' --kind env

# 2. 60 秒・1 回限りのチケットを発行
agentvault issue --secret demo/token --ttl 60s

# 3. 1 回目の resolve は成功、2 回目は拒否
agentvault resolve --ticket av1...
```

---

## これは何で、何ではないか

**これは：**

- ローカル CLI + ローカル broker の **クレデンシャル・ライフサイクル層**。
- MCP 設定の平文 env を **OS キーストア**へ移し、コマンドを `agentvault run ...` に書き換える移行ツール。
- **TTL、max_uses、タスクチケット、子チケット、renew、rotate、revoke** を備えたチケットシステム。
- **JSONL 監査、ハニーポット canary、グローバル失効** を備えたローカル可観測性レイヤー。

**これはではない：**

- サンドボックスではない。同じ OS ユーザーの悪意あるプロセスによるファイル読み取りは止められません。
- ネットワークプロキシではない。egress 制御や DLP は提供しません。
- クラウド KMS ではない。アカウント登録もシークレット送信もありません。
- ゼロトラストの銀の弾丸ではない。ローカルで現実的に運用でき、監査・失効できる中間層です。

---

## 対応エージェント

パス検出は当て推量ではなく、**クライアントごとの静的レジストリ + プロジェクト優先**です。すべての候補は `init --list` と `doctor` に表示され、存在有無、server 数、ラップ済み数が分かります。

| エージェント | 候補設定 | 書き込み形式 |
|---|---|---|
| **Claude Code** | project `.mcp.json` → `~/.claude.json` → `~/.claude/mcp.json` | JSON `mcpServers` |
| **Cursor** | project `.cursor/mcp.json` → `~/.cursor/mcp.json` | JSON `mcpServers` |
| **Codex** | `~/.codex/config.toml` | TOML（inline env、env サブテーブル、複数行 args） |
| **Gemini CLI** | project `.gemini/settings.json` → `~/.gemini/settings.json` | JSON `mcpServers` |
| **Windsurf** | project `.windsurf/mcp.json` → `~/.codeium/windsurf/mcp_config.json` | JSON `mcpServers` |
| **Cline** | VS Code / Cursor / Insiders の `globalStorage/.../cline_mcp_settings.json` | JSON `mcpServers` |
| **Qoder** | project `.qoder/settings.json` → `.qoder/settings.local.json` → `.mcp.json` → `~/.qoder/settings.json` | JSON `mcpServers` |
| **その他** | `--path` で指定した任意の `mcpServers` JSON | JSON `mcpServers` |

> ディスク全体の再帰スキャンはしません。レジストリの候補 + 明示的な `--path` のみです。動作が予測可能で、無関係なファイルを触りません。

未知のフィールド（`type`、`cwd`、`disabled`、`headers`、`timeout`、`trust`、`autoApprove` など）は書き換え時に保持されます。

---

## 5 つのレイヤー

<p align="center">
  <img src="docs/assets/diagrams/security-layers.svg" alt="Security layers" width="100%">
</p>

| レイヤー | 内容 | v0.1 |
|---|---|---|
| **L0 シークレット保管** | OS キーストア、デバイス/ユーザー + ハードウェア指紋、署名付きチケット | ✅ 実装済み |
| **L1 チケット + broker** | コマンドラップ、ローカル broker、短期チケット、タスクチケット、renew、rotate、revoke | ✅ 実装済み |
| **L2 サンドボックス** | プロセス分離 / microVM | ⏳ 将来 |
| **L3 egress 制御** | allowlist / DLP | ⏳ 将来 |
| **L4 監査 + ハニーポット** | 追記型 JSONL、canary、グローバル失効 | ✅ 実装済み |

L0 + L1 で「設定に平文を置かない」「権限に時間の境界がある」「インシデント時に失効できる」を解決します。L2/L3 はより大きなプロジェクトであり、完了したふりはしません。

---

## インストール

### 方法 A：ソースからビルド（現時点の推奨）

stable Rust（edition 2021）が必要です。Windows / macOS / Linux で動作します。

```bash
git clone https://github.com/Seramys/agentvault.git
cd agentvault
cargo build --release
```

成果物：

- Windows：`target\release\agentvault.exe`
- macOS / Linux：`target/release/agentvault`

安定したディレクトリ（`~/.local/bin` や `C:\Tools\agentvault` など）に置き、ユーザー環境変数を永続化します。

```bash
agentvault env install --dry-run   # 変更内容を確認
agentvault env install             # user scope の AGENTVAULT_HOME + PATH
```

> 新しいターミナル / 新しいログインセッションでのみ反映されます。既に開いている shell は再起動してください。

### 方法 B：`cargo install`

```bash
cargo install --path crates/cli
agentvault --version
```

### 方法 C：GitHub Releases

リリース公開後は各プラットフォームのバイナリを取得できます。v0.1 では、このドキュメントと一致するバイナリを得るためソースビルドを主経路とします。

---

## クイックスタート

> 以下の例はすべて `--home` で隔離ディレクトリを指定します。まず安全に試し、確認後に `--home` を外してください。

### 1. 長期シークレットを保存

```bash
agentvault --home "$HOME/.agentvault-demo" secret add demo/token \
  --value 'sk-test-demo-123' --kind env
```

```json
{
  "replaced": false,
  "secret": "demo/token",
  "stored": true,
  "value_changed": false
}
```

### 2. 確認

```bash
agentvault --home "$HOME/.agentvault-demo" secret list
```

```text
demo/token                   kind=env    epoch=0   honeypot=false revoked=false
```

### 3. チケットを発行

```bash
agentvault --home "$HOME/.agentvault-demo" issue --secret demo/token --ttl 60s
```

```text
av1.eyJ2IjoxLCJqdGkiOiI1ZGU5OTY1ZC0...
kind=leaf jti=5de9965d-... expires_in=60s uses=1 hard_exp=...
```

### 4. 1 回だけ使用

```bash
agentvault --home "$HOME/.agentvault-demo" resolve --ticket av1...
```

1 回目は値を返し、2 回目は次を返します。

```text
error: denied: ticket revoked
```

### 5. すべて失効

```bash
agentvault --home "$HOME/.agentvault-demo" revoke --all
```

`revoke --all` はグローバル epoch を進めます。古いチケットは即座にすべて無効になり、以後に発行されたチケットは新しい epoch を使います。

---

## 図解ステップガイド

実際の利用順に沿って説明します。各ステップにコマンド、期待される出力、注意点があります。

### ステップ 0：全体像

<p align="center">
  <img src="docs/assets/diagrams/architecture.svg" alt="Architecture overview" width="100%">
</p>

パイプラインは 1 本です。

```text
init が設定を検出 → dry-run で計画表示 → バックアップ → env 移行 → コマンドをラップ → 検証
```

`init` は「見つけたものを何でも変更」しません。ラップ対象として計画した server だけを触り、URL-only、ラップ済み、解析不能な server は理由付きでスキップします。

### ステップ 1：インストールと確認

```bash
agentvault --version
agentvault doctor
```

```text
home:      C:\Users\you\AppData\Local\agentvault
home from: platform default
default:   C:\Users\you\AppData\Local\agentvault
writable:  yes
exe:       C:\Tools\agentvault\agentvault.exe
config:    missing (not created yet)
broker:    offline
keystore:  windows-dpapi
env:       AGENTVAULT_HOME is not persisted (run `agentvault env install`)
path:      missing (install dir C:\Tools\agentvault)
[x] codex    C:\Users\you\.codex\config.toml (2 server(s), 0 to wrap)
```

確認する 4 点：

- `home`：状態ディレクトリ；
- `keystore`：DPAPI / Keychain / Secret Service / 暗号化ファイル；
- `broker`：ローカル broker がオンラインか；
- 下部の `[x]`：検出したエージェント設定と、ラップ対象の残数。

### ステップ 2：エージェント設定の検出

```bash
agentvault init --list
```

```text
claude:
  [ ] .mcp.json (missing)
  [ ] C:\Users\you\.claude.json (missing)
cursor:
  [ ] .cursor\mcp.json (missing)
  [ ] C:\Users\you\.cursor\mcp.json (missing)
codex:
  [x] C:\Users\you\.codex\config.toml (2 server(s), 0 to wrap)
qoder:
  [x] C:\Users\you\.qoder\settings.json (0 server(s), 0 to wrap, 0 wrapped)

auto would pick: codex (C:\Users\you\.codex\config.toml)
```

ポイント：

- **プロジェクト優先**：リポジトリ内で実行すると、プロジェクトの `.cursor/mcp.json` や `.mcp.json` がユーザーグローバルより先に選ばれます。
- `--agent auto` は存在する候補から選ぶだけで、ファイルを作成しません。
- ファイル指定：`agentvault init --path .\mcp.json`。
- クライアント強制：`agentvault init --agent qoder`。

### ステップ 3：dry-run で何も変更しない

これが最重要の安全弁です。**計画を読んでから書き込みます。**

テスト設定（または実設定のコピー）を用意します。

```json
{
  "mcpServers": {
    "postgres": {
      "command": "npx",
      "args": ["@modelcontextprotocol/server-postgres"],
      "env": {
        "DATABASE_URL": "postgres://demo:demo@localhost/db",
        "API_KEY": "sk-test-demo-123",
        "PASS_THROUGH": "${HOME}/.config",
        "LOG_LEVEL": "info",
        "MAX_TOKENS": "4096"
      }
    }
  }
}
```

実行：

```bash
agentvault init --path .\mcp.json --dry-run --no-env
```

実際の出力：

```text
agent:        generic
config:       .\mcp.json
would wrap 1 server(s):
  - postgres: npx @modelcontextprotocol/server-postgres -> agentvault run --name postgres
    migrate env: API_KEY, DATABASE_URL
    keep env:    PASS_THROUGH (reference/override)
    unchanged env: LOG_LEVEL, MAX_TOKENS
dry run: no files were changed
```

<p align="center">
  <img src="docs/assets/diagrams/env-migration.svg" alt="Env migration decision flow" width="100%">
</p>

4 つのバケット：

| 出力 | 意味 | 設定への影響 |
|---|---|---|
| `migrate env` | シークレットに見える | 値をキーストアへ保存、env 行を削除し、実行時に `--env KEY=secret:NAME` で注入 |
| `keep env` | `${...}` / `$VAR` / `%VAR%` / `{{...}}` 参照、または `--keep-env` 指定 | そのまま保持し、MCP クライアントが展開 |
| `unchanged env` | 通常の設定 | そのまま保持、保存しない |
| `skip` | URL-only、ラップ済み、解析不能 | 一切変更しない |

> **注意**：参照検出は埋め込み参照も対象になりました。`${HOME}/.config`、`$HOME/.config`、`%USERPROFILE%\...`、`prefix-${ENV}-suffix` などです。以前は「値全体が参照」の場合しか認識せず、`${HOME}/.config` を誤って移行していました。修正済みで単体テストもあります。

誤判定した場合は明示的に上書きできます。

```bash
# 普通に見える key でも強制移行
agentvault init --path .\mcp.json --migrate-env SESSION_ID

# シークレットに見えても強制保持
agentvault init --path .\mcp.json --keep-env INTERNAL_HINT
```

### ステップ 4：init を実行する

dry-run の計画に問題がなければ：

```bash
agentvault init --path .\mcp.json
```

起こること：

1. ローカル broker を確保（`--no-autostart` で無効化）；
2. タイムスタンプ付きバックアップ `mcp.json.bak.<timestamp>` を書き込み；
3. 計画された env 値だけをキーストアへ移行；
4. コマンドを `agentvault run --name postgres -- ...` に書き換え；
5. 設定を書き戻し、再解析で検証；
6. 書き込み先とバックアップ先を表示。

<p align="center">
  <img src="docs/assets/diagrams/init-flow.svg" alt="init flow" width="100%">
</p>

ラップ後の JSON：

```json
{
  "mcpServers": {
    "postgres": {
      "command": "C:\\Tools\\agentvault\\agentvault.exe",
      "args": [
        "--home", "C:\\Users\\you\\AppData\\Local\\agentvault",
        "run", "--name", "postgres",
        "--env", "API_KEY=secret:postgres/API_KEY",
        "--env", "DATABASE_URL=secret:postgres/DATABASE_URL",
        "--",
        "npx", "@modelcontextprotocol/server-postgres",
        "postgres://demo:demo@localhost/db"
      ],
      "env": {
        "PASS_THROUGH": "${HOME}/.config",
        "LOG_LEVEL": "info",
        "MAX_TOKENS": "4096"
      }
    }
  }
}
```

知っておくべき点：

- ラップ後の `command` は **絶対パス**（`launcher_path()`）で、裸の `agentvault` ではありません。エージェントの PATH に依存しません。
- ラッパーは明示的な `--home` を持ち、shell ごとに別の vault を読む事故を防ぎます。
- 通常の env は保持され、計画された key だけが削除されます。
- 2 回目の `init` は no-op（冪等）です。二重ラップや再移行はありません。
- 移行値は実行時に `--env KEY=secret:NAME` で解決されるため、MCP server 側のコード変更は不要です。

### ステップ 5：最初のチケットを発行する

```bash
agentvault issue --secret postgres/DATABASE_URL --ttl 15m --scope read
```

```text
av1.eyJ2IjoxLCJqdGkiOiI...
kind=leaf jti=... expires_in=900s uses=1 hard_exp=...
```

`av1` チケットは HMAC-SHA256 で署名されます。claims には次が含まれます。

- `jti`：監査と失効に使うチケット ID；
- `sid`：secret ID；
- `scope`：権限範囲（既定 `read`）；
- `iat` / `exp` / `hard_exp`；
- `epoch`（secret ローテーション世代）と `gepoch`（グローバル失効世代）；
- `max_uses` / `kind` / `parent` / `renewable`。

スクリプト向けに `--json` も使えます。

```bash
agentvault issue --secret postgres/DATABASE_URL --ttl 15m --json
```

### ステップ 6：1 回で使用、再生は拒否

```bash
TICKET=$(agentvault issue --secret postgres/DATABASE_URL --ttl 60s)
agentvault resolve --ticket "$TICKET"   # 成功、値を返す
agentvault resolve --ticket "$TICKET"   # 拒否
```

<p align="center">
  <img src="docs/assets/terminal/replay-denied.svg" alt="Replay denied" width="100%">
</p>

実際の 2 回目：

```text
error: denied: ticket revoked
```

これはクライアントのバグではなく、仕様です。既定は `max_uses=1` で、使うと失効します。複数回必要な場合は明示します。

```bash
agentvault issue --secret postgres/DATABASE_URL --max-uses 3 --ttl 5m
```

### ステップ 7：タスクチケットと子チケット

2 時間のエージェントタスクに、2 時間有効で再利用可能な本番キーを持たせるべきではありません。

```bash
# 親：短い TTL、renew 可能、2 時間の hard cap
PARENT=$(agentvault issue --secret postgres/DATABASE_URL \
  --task --ttl 30s --hard-ttl 2h --json | jq -r .ticket)

# 各ツール呼び出し前に 60 秒の葉チケットを発行
CHILD=$(agentvault child --parent "$PARENT" --ttl 60s)
agentvault resolve --ticket "$CHILD"
```

ルール：

- 子の期限は親の `hard_exp` を超えられません。
- `renew` は親の `exp` を後ろへ動かせますが、`hard_ttl` は超えられません。
- 親を失効すると子も無効になります。
- `run --session` は renew 可能な親を作り、子プロセス終了時に失効させます。

<p align="center">
  <img src="docs/assets/diagrams/ticket-lifecycle.svg" alt="Ticket lifecycle" width="100%">
</p>

### ステップ 8：実プロセスをラップする

```bash
agentvault run --name postgres -- npx @modelcontextprotocol/server-postgres
```

複数の移行済みシークレットを同時に注入：

```bash
agentvault run --name postgres \
  --env DATABASE_URL=secret:postgres/DATABASE_URL \
  --env API_KEY=secret:postgres/API_KEY \
  -- npx @modelcontextprotocol/server-postgres
```

挙動：

- `--name NAME`：**完全同名**の secret があれば、子プロセスに `AGENTVAULT_TICKET` をエクスポート。
- `--env KEY=secret:NAME`：起動前に `NAME` を解決し、通常の env として値を注入。
- `--session`：renew 可能な親を作り、次をエクスポート：
  - `AGENTVAULT_PARENT_TICKET`
  - `AGENTVAULT_PARENT_FILE`
- 子プロセス終了後、session の親は失効し、バックグラウンド renew も停止します。

> `init` が `AGENTVAULT_TICKET` を自動エクスポートしない理由：ほとんどの MCP server はその変数を知らず、`DATABASE_URL` などの通常 env しか読みません。`init` は互換性のある「実行時に値を注入」する経路を使います。チケット対応の custom server は `run --name <同名 secret>` または `--session` を使ってください。

### ステップ 9：アップグレード後に launcher を更新

debug ビルドから release へ移行、またはインストール先を移動した場合、env の再移行は不要です。

```bash
agentvault init --agent codex --refresh --dry-run
agentvault init --agent codex --refresh
```

<p align="center">
  <img src="docs/assets/terminal/refresh-launcher.svg" alt="Refresh launcher" width="100%">
</p>

変更するのは次の 2 つだけです。

- ラップ済み server の launcher パス；
- `--home` の値。

env、`run` 以降の元コマンド、`--no-autostart` などのサフィックスは**変更しません**。JSON と Codex TOML に対応し、各ファイルは先にバックアップされます。

### ステップ 10：退職・端末紛失・ハニーポット

**グローバル kill switch：**

```bash
agentvault revoke --all
```

**1 つの secret の新規発行を停止：**

```bash
agentvault revoke --secret postgres/DATABASE_URL
```

**1 つのチケットを失効（jti または完全な token）：**

```bash
agentvault revoke --ticket 5de9965d-...
agentvault revoke --ticket av1...
```

**ローテーション：**

```bash
agentvault rotate --secret postgres/DATABASE_URL
```

新しい値を生成し、secret epoch を進めて古いチケットを resolve 時に失敗させ、その secret の失効状態を解除します。

**ハニーポット：**

```bash
agentvault honeypot add prod/canary --json
# 返された token を、エージェントが絶対に読むべきでない .env / 設定に置く
agentvault honeypot check <token>
```

<p align="center">
  <img src="docs/assets/terminal/honeypot-trip.svg" alt="Honeypot trip" width="100%">
</p>

実際の出力：

```text
hit=true honeypot=prod/canary revoked_all=true
```

ヒットするとグローバル失効が発生します。以後の resolve は次を返します。

```text
error: denied: ticket was invalidated by a global revocation
```

**監査：**

```bash
agentvault audit --tail 20
```

```text
[1795261677681] ticket.resolve     actor=cli  secret=demo/token  ticket=a705ff08-...  ok
[1795261677724] ticket.resolve     actor=cli  secret=demo/token  ticket=a705ff08-...  revoked
[1795261686286] honeypot.triggered actor=cli  secret=canary      ticket=-             blocked
```

監査は追記型 JSONL です。ローテーション時は以前のファイルを保持します。

### ステップ 11：実テストを実行

```bash
cargo test --workspace
cargo build --release
```

ローカル結果（Windows 11、release バイナリ）：

```text
e2e:        9 passed
adapters:  16 passed
audit:      1 passed
core:      10 passed
keystore:   2 passed
total:     38 passed, 0 failed
```

<p align="center">
  <img src="docs/assets/diagrams/test-evidence.svg" alt="Test evidence" width="100%">
</p>

カバー内容はハッピーパスではありません。

- JSON / Codex / Qoder 設定の書き換えと冪等性；
- 未知フィールド保持、複数行 TOML args；
- ワンタイム、期限切れ、再生拒否；
- task の hard-TTL 上限、renew が短縮しないこと；
- revoke token / jti / all、rotate 後の旧チケット失効；
- ハニーポットによるグローバル失効；
- autostart 切替、単一インスタンス、broken pipe；
- 実際の Codex 設定コピーに対する refresh；
- user scope の `env install`、冪等性、アンインストールと復元。

> Cursor/Qoder の GUI 内部ツール呼び出し自動化、macOS/Linux 実機検証、sandbox/egress はありません。これらは既知の限界に記載しており、完了したとは主張しません。

---

## 設定検出と env 移行

### パス検出：レジストリであり、ヒューリスティックではない

- 各クライアントの候補は**静的レジストリ**で、プロジェクトパスがユーザーグローバルより先です。
- `init --list` は全候補、存在有無、server 数、ラップ済み数、`auto` の選択を表示します。
- `--agent` でクライアントを強制、`--path` でファイル指定。
- 再帰スキャン、ブラウザ履歴、当て推量はありません。

### env 移行：ヒューリスティックだが可視・上書き可能

判定順：

1. **参照ガード**：`${...}`、`$VAR`、`%VAR%`、`{{...}}`、および `${HOME}/.config` のような埋め込み → 絶対に移行しない。
2. **key マーカー**：`TOKEN`、`SECRET`、`PASSWORD`、`API_KEY`、`DATABASE_URL`、`DSN`、`WEBHOOK`、`COOKIE` など。
3. **値の形**：`sk-`、`ghp_`、JWT、PEM、`postgres://user:pass@...`、長いランダム文字列など。

ヒューリスティックは間違えます。だから 3 つの安全弁があります。

```bash
agentvault init --dry-run                 # 確認
agentvault init --migrate-env KEY         # 強制移行
agentvault init --keep-env KEY            # 強制保持
```

`init --dry-run` は `migrate env` / `keep env` / `unchanged env` / `skip` を表示します。実際の書き込みは必ず先にバックアップします。

### バックアップと失敗時の復元

- 書き込み前：`mcp.json.bak.<timestamp>` / `config.toml.bak.<timestamp>`。
- 書き込み後：再解析してラップ結果を検証し、失敗時はバックアップから復元。
- バックアップには元の平文が含まれます。新しい設定を確認したら削除してください。
- `init --refresh` もバックアップしますが、env は触りません。

---

## コマンド一覧

| コマンド | 目的 | 主なオプション |
|---|---|---|
| `init` | MCP 設定を検出・ラップし、secret env を移行 | `--dry-run` `--list` `--refresh` `--agent` `--path` `--migrate-env` `--keep-env` `--no-env` `--rebind` |
| `doctor` | home、broker、keystore、agent 設定を診断 | `--home` |
| `env show` | home の由来と永続化値を表示 | `--home` |
| `env install` | `AGENTVAULT_HOME` + PATH を永続化 | `--scope user\|machine` `--no-path` `--dry-run` |
| `env uninstall` | 永続化値を削除 | `--scope user\|machine` `--dry-run` |
| `secret add` | 長期シークレットを保存 | `--value` `--stdin` `--kind` `--honeypot` |
| `secret list` | シークレット一覧（値は表示しない） | `--home` |
| `secret remove` | シークレット削除 | `<NAME>` |
| `secret unrevoke` | ローテーションせず失効名の再発行を許可 | `<NAME>` |
| `issue` | 短期チケットを発行 | `--secret` `--ttl` `--scope` `--max-uses` `--task` `--hard-ttl` `--json` |
| `resolve` | チケットを交換（既定で 1 回） | `--ticket` `--json` |
| `child` | 親から葉チケットを発行 | `--parent` / `--parent-file` `--ttl` `--sub` `--max-uses` |
| `renew` | 親の期限を延長、hard-ttl が上限 | `--ticket` `--ttl` |
| `run` | プロセスをラップし、チケットまたは解決値を注入 | `--name` `--env KEY=secret:NAME` `--session` `--ttl` `--hard-ttl` |
| `revoke` | チケット / secret / すべてを失効 | `--ticket` `--secret` `--all` |
| `rotate` | 値をローテーションし旧チケットを無効化 | `--secret` |
| `status` | secret、バインド済み agent、live ticket を表示 | `--json` |
| `audit` | JSONL 監査を表示 | `--tail N` `--json` |
| `honeypot add/list/check` | ハニーポット canary を管理 | `--json` |
| `daemon` | broker をフォアグラウンド実行 | `--detach` |
| `stop` | バックグラウンド broker を停止 | `--home` |

broker が必要なコマンドは既定で detached broker を自動起動します。`--no-autostart` または `AGENTVAULT_NO_AUTOSTART=1` で無効化できます。

---

## 初回実行と AGENTVAULT_HOME

既定の状態ディレクトリ：

| プラットフォーム | パス |
|---|---|
| Windows | `%LOCALAPPDATA%\agentvault` |
| macOS | `~/Library/Application Support/agentvault` |
| Linux | `$XDG_DATA_HOME/agentvault` または `~/.local/share/agentvault` |

優先順位：`--home` > `AGENTVAULT_HOME` > プラットフォーム既定。

最初の実際の `init`（`--home` 上書きではなくプラットフォーム既定を使用）では：

- `AGENTVAULT_HOME` を **user scope** に永続化；
- バイナリディレクトリを **user PATH** に追加；
- machine-wide 変数を黙って書きません。必要なら `env install --scope machine` を明示（通常は昇格が必要）。

管理：

```bash
agentvault env show
agentvault env install --dry-run
agentvault env install
agentvault env install --scope machine
agentvault env uninstall
agentvault init --no-env   # 初回 init で環境変数を触らない
```

注意：

- 既に開いているターミナルには反映されません。新しいターミナル / 再ログイン後に有効になります。
- `--home` または `AGENTVAULT_HOME` を使った場合、初回 init はその一時パスをグローバル既定として永続化しません。
- ラップ済み server は明示的な `--home` を持つため、agent の PATH/env が違っても同じ vault を読みます。

---

## 実テストの証拠

これは「いくつかの単体テストを書いて完了」ではありません。実際の Windows 11 マシンで：

- **38 件の Rust テスト**：`cargo test --workspace` 全通過。
- **150 件の隔離 CLI チェック**：それぞれ独立した `--home` を使い、init、ticket、revoke、rotate、env、doctor、autostart、broken pipe をカバー。
- **実際の Codex 設定コピー**：実 `config.toml` をコピーし、テスト server を追加して init/refresh を実行、`codex mcp get` で launcher と home を検証。
- **実際の Cursor / Qoder 探索**：パスレジストリ、空設定、プロジェクト優先を確認。
- **実際の環境変数書き込みと復元**：user scope の `env install`、冪等性、`env uninstall` 復元。
- **実際の攻撃セマンティクス**：再生拒否、期限切れ拒否、グローバル失効、ローテーション無効化、ハニーポット発火。

<p align="center">
  <img src="docs/assets/terminal/replay-denied.svg" alt="Replay denied" width="100%">
</p>

<p align="center">
  <img src="docs/assets/terminal/honeypot-trip.svg" alt="Honeypot trip" width="100%">
</p>

---

## セキュリティモデル

<p align="center">
  <img src="docs/assets/diagrams/security-layers.svg" alt="Security model" width="100%">
</p>

### L0：シークレット保管

- Windows：DPAPI（PowerShell 経由。追加の native ビルド依存なし）。
- macOS：Keychain。
- Linux：Secret Service。headless では 0600 の暗号化ファイルにフォールバック。
- `status` が現在の backend を表示します。

### デバイスとハードウェアのバインディング

- デバイス指紋：hostname + user。
- `init --rebind` でハードウェア指紋（WMI UUID / IOPlatformUUID / DMI product UUID）を記録。
- 不一致なら broker は起動を拒否。
- TPM sealing ではありません。TPM は将来の enterprise 方向です。

### チケット

- HMAC-SHA256。キーは master key + グローバル失効 epoch から派生。
- `revoke --all` はグローバル epoch を進め、古いチケットを一括失効。
- secret ローテーションは secret epoch を進め、古いチケットは resolve で失敗。
- 既定 `max_uses=1`。使い切ると自動失効。
- task チケットは `hard_exp` を持ち、`renew` は越えられません。
- 子チケットの期限は親の `hard_exp` が上限。
- `revoke --ticket` は jti または完全な token を受け付けます。

### ローカル broker

- `127.0.0.1` のみにバインド。
- 単一インスタンスロック。
- `admin.token` は owner-only ACL（Windows は `icacls`、Unix は 0600）。
- 復号済み secret はメモリにキャッシュ。毎回 PowerShell を起動しません。
- broker 終了後、次に必要なコマンドが自動起動します。

### 監査とハニーポット

- 追記型 JSONL：時刻、actor、イベント、secret、ticket、結果。
- `audit --tail N` で表示。
- ローテーション時は古い監査ファイルを保持。
- ハニーポット token は一意で、ヒットするとグローバル失効。
- ハニーポット check が本物の secret を漏らすことはありません。

---

## 既知の限界

v0.1 の実際の限界です。脚注に隠しません。

1. **サンドボックスではない**：同じ OS ユーザーの悪意あるプロセスは、ユーザーファイル、`admin.token`、バックアップを読めます。
2. **broker はローカル TCP**：リモートサービスではありません。`admin.token` を読めるローカルプロセスは管理できます。
3. **デバイスバインディングは TPM ではない**：指紋チェックであり、ハードウェア封印ではありません。
4. **未移行の平文は対象外**：v0.1 は既知の設定のみ扱います。スクリプト、`.env`、ログの平文は対象外です。
5. **env 移行はヒューリスティック**：key マーカー + 値の形で、誤判定します。dry-run と `--migrate-env` / `--keep-env` を使ってください。
6. **Codex TOML は限定的**：quoted server names、inline/env サブテーブル、複数行 args に対応。`.env` 以外の bare nested table は無視されます。
7. **JSON は strict 必須**：JSONC コメント/末尾カンマは明確なエラーで拒否。UTF-8 BOM は許可。
8. **`~/.claude.json` はトップレベルのみ**：`projects.*.mcpServers` は自動書き換えしません。プロジェクトの `.mcp.json` に `--path` を使ってください。
9. **`init` は既定で値注入、チケットではない**：stock MCP server と互換。チケット対応は `run --name <同名 secret>` または `--session`。
10. **GUI 自動化なし**：Cursor/Qoder の GUI 内部ツール呼び出しは自動化していません。CLI と設定レイヤーは検証済みです。
11. **macOS/Linux 実機検証は未実施**：コードと単体テストはクロスプラットフォームですが、実機実行は Windows 11 です。
12. **Web コンソールは延期**：このリポジトリは CLI のみ。web コードは今回のリリースに含みません。

---

## FAQ

### Q1：システム環境変数に token を置くのと何が違う？

システム環境変数は長期平文で、同じユーザーの任意プロセスが読め、TTL・回数制限・失効・監査がありません。AgentVault は長期値を OS キーストアに置き、エージェントには期限付きで失効可能な短期チケットを渡します。

### Q2：`init` で設定が壊れない？

dry-run で計画、バックアップ、計画 server のみ変更、書き込み後の再解析、失敗時の復元を行います。未知フィールドも保持します。それでも本番設定は先にコピーするか、`--path` でコピーに対して試してください。

### Q3：なぜ `${HOME}/.config` は移行されない？

意図的です。参照でありシークレットではありません。移行すると展開がリテラルになり、MCP クライアントの動作を壊します。`keep env` に表示されます。

### Q4：`init` 後、なぜチケットではなく値が渡る？

ほとんどの MCP server は `DATABASE_URL` / `API_KEY` などの通常 env しか読みません。`init` は互換経路として、起動時にチケットを値に交換して env に注入します。チケット対応の custom server は `run --name <同名 secret>` または `--session` を使ってください。

### Q5：`revoke --all` 後も使える？

使えます。グローバル失効は epoch を進め、以後に発行されたチケットは新しい epoch で正常に動きます。古いチケットはすべて無効です。

### Q6：revoke と rotate の違いは？

- `revoke --secret`：その secret の新規発行を停止。既存チケットも失敗。値は残る。
- `rotate --secret`：新しい値を生成し epoch を進め、古いチケットを無効化し、その secret の失効状態を解除。
- `revoke --all`：全 secret の全旧チケットを一括無効化。

### Q7：broker は毎回のツール呼び出しを遅くする？

broker は復号済み secret をメモリにキャッシュし、DPAPI は起動時に 1 回だけ呼びます。通常の issue/resolve はローカル TCP + HMAC 検証で軽量です。

### Q8：自分の MCP server に接続できる？

はい。2 通り：

```bash
# 1. 通常 env 注入（stock server 互換）
agentvault run --name my-server --env API_KEY=secret:my/API_KEY -- node server.js

# 2. チケット対応
agentvault run --name my/API_KEY -- node server.js
# 子プロセスは AGENTVAULT_TICKET を読み、agentvault resolve を呼ぶ
```

### Q9：なぜ Web コンソールを先に作らない？

CLI が中核であり、最小の信頼面です。設定移行、チケット意味論、失効、監査を先に堅牢にします。このリポジトリは CLI のみを公開します。

### Q10：チームで 1 つの vault を共有できる？

v0.1 は単一マシン・単一ユーザーのローカル vault です。チーム共有、中央ポリシー、リモート監査は将来の課題です。同じ home ディレクトリをネットワーク共有に置かないでください。

---

## リポジトリ構成

```text
crates/
  core/      domain types, token signing, paths, JSON store
  keystore/  OS keystore + encrypted-file fallback + derivation
  audit/     append-only JSONL audit
  adapters/  JSON mcpServers + Codex TOML discovery and rewrite
  broker/    local TCP broker, ticket lifecycle, client
  cli/       agentvault binary
docs/
  assets/    logo, diagrams, terminal evidence
```

主な入口：

- `crates/cli/src/main.rs`：コマンド解析、init/issue/run/doctor など。
- `crates/adapters/src/lib.rs`：パスレジストリ、JSON の plan/apply/refresh、env ヒューリスティック。
- `crates/adapters/src/codex.rs`：Codex TOML の解析と書き換え。
- `crates/broker/src/lib.rs`：ローカル broker とチケットライフサイクル。
- `crates/core/src/token.rs`：HMAC チケットの署名/検証。
- `crates/keystore/src/lib.rs`：DPAPI / Keychain / Secret Service / file backend。

---

## ロードマップ

- [x] L0：OS キーストア、デバイスバインディング、ハードウェア指紋、署名付きチケット
- [x] L1：ローカル broker、短期チケット、task/child/renew、rotate/revoke
- [x] L4：JSONL 監査、ハニーポット canary、グローバル失効
- [x] `init --refresh`：アップグレード後の launcher/home 更新
- [x] 埋め込み env 参照ガード（`${HOME}/...`、`%VAR%\...`）
- [ ] L2：プロセスサンドボックス / microVM
- [ ] L3：egress allowlist / DLP
- [ ] Web コンソール（別リポジトリ / 将来バージョン）
- [ ] macOS/Linux 実機検証と CI カバレッジ
- [ ] TPM / Secure Enclave バインディング
- [ ] チームポリシーと中央監査

---

## コントリビュートとセキュリティ

- コントリビュートガイド：[CONTRIBUTING.md](CONTRIBUTING.md)
- セキュリティポリシーと脆弱性報告：[SECURITY.md](SECURITY.md)
- issue / PR に実際の token、cookie、本番設定を貼らないでください。`sk-test-...` の合成値を使ってください。
- 設定書き換えは必ずバックアップを保持し、`*.bak.*` を commit しないでください。
- 中国語 README が正とします。動作変更時は EN/JA/KO も更新してください。

---

## ライセンス

Apache License 2.0。[LICENSE](LICENSE) を参照してください。

---

<p align="center">
  <sub>Built for agents that should borrow credentials, not own them.</sub>
</p>

<p align="center">
  <a href="https://star-history.com/#Seramys/agentvault&Date">
    <img src="https://api.star-history.com/svg?repos=Seramys/agentvault&type=Date" alt="Star history" width="720">
  </a>
</p>