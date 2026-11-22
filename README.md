<p align="center">
  <img src="docs/assets/brand/logo.png" width="150" alt="AgentVault logo">
</p>

<h1 align="center">AgentVault</h1>

<p align="center">
  <strong>把 AI Agent 的长期密钥，变成一次性票据。</strong><br>
  长期密钥留在 OS keystore；Agent 拿到的是会过期、可撤销、默认只能用一次的票据。
</p>

<p align="center">
  <a href="https://github.com/Seramys/agentvault/actions/workflows/ci.yml"><img src="https://github.com/Seramys/agentvault/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-6D5BD0.svg" alt="License"></a>
  <img src="https://img.shields.io/badge/rust-stable-CE422B.svg" alt="Rust stable">
  <img src="https://img.shields.io/badge/status-v0.1%20%C2%B7%20web-0F9F6E.svg" alt="v0.1 · web">
</p>

<p align="center">
  <b>简体中文</b> · <a href="README_EN.md">English</a> · <a href="README_JA.md">日本語</a> · <a href="README_KO.md">한국어</a>
</p>

<p align="center">
  <a href="#30-秒理解">30 秒理解</a> ·
  <a href="#快速开始">快速开始</a> ·
  <a href="#web-控制台">Web 控制台</a> ·
  <a href="#图文操作指南">图文操作指南</a> ·
  <a href="#命令手册">命令手册</a> ·
  <a href="#安全模型">安全模型</a> ·
  <a href="#真实测试证据">测试证据</a>
</p>

> **当前范围**：L0 密钥托管、L1 短期票据与本地 broker、L4 审计/蜜罐，以及本地 Web 控制台（账号与角色、租户标签、SSE 实时刷新、审计签名校验、加密备份）。L2 沙箱与 L3 出站控制/DLP 仍在后续版本。
>
> **一句话**：你不需要再决定“这个 token 要不要写进 `mcp.json`”。先存进 vault，让 agent 用票据来换；需要看全局时，打开本地控制台。

---

## 目录

<details>
<summary>展开目录</summary>

- [它解决什么问题](#它解决什么问题)
- [30 秒理解](#30-秒理解)
- [它是什么，不是什么](#它是什么不是什么)
- [支持哪些 Agent](#支持哪些-agent)
- [五层模型](#五层模型)
- [安装](#安装)
- [快速开始](#快速开始)
- [Web 控制台](#web-控制台)
- [加密备份、签名审计与 TOML 往返](#加密备份签名审计与-toml-往返)
- [图文操作指南](#图文操作指南)
  - [第 0 步：先看全景](#第-0-步先看全景)
  - [第 1 步：安装并确认环境](#第-1-步安装并确认环境)
  - [第 2 步：发现 Agent 配置](#第-2-步发现-agent-配置)
  - [第 3 步：dry-run，不改任何文件](#第-3-步dry-run不改任何文件)
  - [第 4 步：真正执行 init](#第-4-步真正执行-init)
  - [第 5 步：发出第一张票据](#第-5-步发出第一张票据)
  - [第 6 步：一次使用，重放被拒](#第-6-步一次使用重放被拒)
  - [第 7 步：长任务票据与子票据](#第-7-步长任务票据与子票据)
  - [第 8 步：包装一个真实进程](#第-8-步包装一个真实进程)
  - [第 9 步：升级后刷新 launcher](#第-9-步升级后刷新-launcher)
  - [第 10 步：离职、丢失设备与蜜罐](#第-10-步离职丢失设备与蜜罐)
  - [第 11 步：跑真实测试](#第-11-步跑真实测试)
- [配置识别与 env 迁移](#配置识别与-env-迁移)
- [命令手册](#命令手册)
- [首次运行与 AGENTVAULT_HOME](#首次运行与-agentvault_home)
- [真实测试证据](#真实测试证据)
- [安全模型](#安全模型)
- [已知边界](#已知边界)
- [常见问题](#常见问题)
- [项目结构](#项目结构)
- [路线图](#路线图)
- [贡献与安全](#贡献与安全)
- [许可证](#许可证)

</details>

---

## 它解决什么问题

把 Claude Code、Cursor、Codex、Qoder 这类 MCP 客户端接上数据库、GitHub、浏览器或内部 API 时，最常见的做法是把密钥直接写进配置：

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

这会带来三个很实际的问题：

1. **配置就是明文密钥库**：`mcp.json`、`config.toml` 被复制、截图、提交到 git，密钥就跟着走。
2. **权限没有时间边界**：agent 一旦拿到值，就永久可用；想让它只读 15 分钟，只能靠人自觉。
3. **出事无法回答“谁在什么时候用了哪个密钥”**：没有审计，没有撤销，没有一次性语义。

AgentVault 把流程改成：

```text
长期密钥  ──►  OS keystore  ──►  短期票据（默认 1 次）  ──►  agent 进程
   ▲                                     │
   └──────── 审计 / 撤销 / 轮换 ◄────────┘
```

配置里不再出现明文；agent 拿到的是 `av1...` 票据。票据过期、用完、被撤销，或者全局 kill switch 触发，都会在 resolve 时被拒绝。

---

## 30 秒理解

<p align="center">
  <img src="docs/assets/diagrams/architecture.svg" alt="AgentVault architecture" width="100%">
</p>

三个角色：

| 角色 | 做什么 | 不做什么 |
|---|---|---|
| **Vault** | 在 OS keystore 中保管长期密钥；签发/校验票据 | 不把长期密钥交给 agent |
| **Broker** | 只监听 `127.0.0.1`；处理 issue/resolve/revoke/rotate | 不对外网开放；不是远程服务器 |
| **Agent wrapper** | `agentvault run` 启动 MCP server，注入票据或短期值 | 不修改原 MCP server 的协议 |
| **Web console** | 读取同一个 home/broker，展示凭证、票据、审计、蜜罐 | 默认只监听 loopback；不替代 CLI 自动化 |

最短的一次体验：

```bash
# 1. 存一个长期密钥（值不会进入配置）
agentvault secret add demo/token --value 'sk-test-demo-123' --kind env

# 2. 发一张 60 秒、只能用一次的票据
agentvault issue --secret demo/token --ttl 60s

# 3. 第一次 resolve：拿到值；第二次：被拒绝
agentvault resolve --ticket av1...
```

想先看界面而不是命令行：

```bash
agentvault web --port 8787
# 浏览器打开 http://127.0.0.1:8787
# 第一次用 AGENTVAULT_HOME/web.token 登录
```

---

## 它是什么，不是什么

**它是：**

- 一个本地 CLI + 本地 broker 的 **credential lifecycle layer**。
- 一个把 MCP 配置中的明文 env **迁移到 OS keystore**、把命令改写成 `agentvault run ...` 的迁移工具。
- 一个带 **TTL、max_uses、任务票据、子票据、renew、rotate、revoke** 的票据系统。
- 一个带 **JSONL 审计、Ed25519 审计签名、蜜罐 canary、全局撤销** 的本地可观测层。
- 一个 **本地 Web 控制台**：单二进制嵌入 React 前端，支持凭证、票据、Agent 接入、审计、蜜罐、账号角色与租户标签；默认只监听 `127.0.0.1`。
- 一个把配置备份变成 **ChaCha20-Poly1305 加密文件** 的备份工具，避免“为了安全迁移，先留下一份明文 `.bak`”。

**它不是：**

- 不是 sandbox，不会阻止同 OS 用户下的恶意进程读取文件。
- 不是网络代理，不做 egress control / DLP。
- 不是云端密钥管理服务，不需要注册账号，也不把密钥上传。
- 不是“零信任银弹”。它是一个可在本机落地、可审计、可撤销的中间层。

---

## 支持哪些 Agent

路径发现不是“猜目录”，而是**静态的 per-client 注册表 + 项目优先**。每个候选路径都会在 `init --list` 和 `doctor` 中显示是否存在、包含多少 server、多少已经包装。

| Agent | 候选配置 | 写入方式 |
|---|---|---|
| **Claude Code** | 项目 `.mcp.json` → `~/.claude.json` → `~/.claude/mcp.json` | JSON `mcpServers` |
| **Cursor** | 项目 `.cursor/mcp.json` → `~/.cursor/mcp.json` | JSON `mcpServers` |
| **Codex** | `~/.codex/config.toml` | TOML（inline env、env 子表、多行 args） |
| **Gemini CLI** | 项目 `.gemini/settings.json` → `~/.gemini/settings.json` | JSON `mcpServers` |
| **Windsurf** | 项目 `.windsurf/mcp.json` → `~/.codeium/windsurf/mcp_config.json` | JSON `mcpServers` |
| **Cline** | VS Code / Cursor / Insiders 的 `globalStorage/.../cline_mcp_settings.json` | JSON `mcpServers` |
| **Qoder** | 项目 `.qoder/settings.json` → `.qoder/settings.local.json` → `.mcp.json` → `~/.qoder/settings.json` | JSON `mcpServers` |
| **其他 MCP 客户端** | 任意 `mcpServers` JSON，用 `--path` 指定 | JSON `mcpServers` |

> 没有“递归扫描整个磁盘”。只有注册表里的候选路径 + 显式 `--path`。这样行为可预测，也避免误改无关文件。

未知字段（`type`、`cwd`、`disabled`、`headers`、`timeout`、`trust`、`autoApprove` 等）在重写时会保留，不会把配置“洗”成只剩命令和参数。

---

## 五层模型

<p align="center">
  <img src="docs/assets/diagrams/security-layers.svg" alt="Security layers" width="100%">
</p>

| 层 | 内容 | v0.1 |
|---|---|---|
| **L0 密钥托管** | OS keystore；设备/用户 + 硬件指纹绑定；票据签名 | ✅ 已实现 |
| **L1 票据与 broker** | 命令包装、本地 broker、短期票据、任务票据、renew、rotate、revoke | ✅ 已实现 |
| **L2 沙箱** | 进程隔离 / microVM | ⏳ 后续 |
| **L3 出站控制** | egress allowlist / DLP | ⏳ 后续 |
| **L4 审计与蜜罐** | 追加式 JSONL、canary、触发全局撤销 | ✅ 已实现 |

L0 + L1 已经能解决“配置里不出现长期密钥、权限有时间边界、出事可撤销”这三件事。L2/L3 是更大工程，不在 v0.1 里假装完成。

---

## 安装

### 方式 A：从源码构建（当前推荐）

要求：stable Rust（edition 2021），Windows / macOS / Linux 均可。要带 Web 控制台，还需要 Node.js 20+ 构建一次前端；只做 CLI 可以跳过 Node。

```bash
git clone https://github.com/Seramys/agentvault.git
cd agentvault

# 含 Web 控制台：前端资源在编译时嵌入二进制
cd webapp && npm ci && npm run build && cd ..

cargo build --release
```

产物：

- Windows：`target\release\agentvault.exe`
- macOS / Linux：`target/release/agentvault`

把二进制放到一个稳定目录（例如 `~/.local/bin` 或 `C:\Tools\agentvault`），再用 `env install` 写入用户级 PATH：

```bash
agentvault env install --dry-run   # 先看会改什么
agentvault env install             # 写入用户级 AGENTVAULT_HOME + PATH
```

> 新开的终端 / 新登录的会话才会看到环境变量变化；已经开着的 shell 需要重启。

### 方式 B：`cargo install`

```bash
cargo install --path crates/cli
agentvault --version
```

### 方式 C：GitHub Release

Release 页有 v0.1.0 的 CLI 二进制；Web 控制台、加密备份、签名审计等新功能请先从源码构建，下一版发布后再提供对应安装包。

---

## 快速开始

> 下面所有示例都使用 `--home` 指向一个隔离目录，方便你先试，不碰真实配置。确认后再去掉 `--home`。

### 1. 存一个长期密钥

```bash
agentvault --home "$HOME/.agentvault-demo" secret add demo/token \
  --value 'sk-test-demo-123' --kind env
```

你会看到类似：

```json
{
  "replaced": false,
  "secret": "demo/token",
  "stored": true,
  "value_changed": false
}
```

### 2. 看它存在哪

```bash
agentvault --home "$HOME/.agentvault-demo" secret list
```

```text
demo/token                   kind=env    epoch=0   honeypot=false revoked=false
```

### 3. 发一张票据

```bash
agentvault --home "$HOME/.agentvault-demo" issue --secret demo/token --ttl 60s
```

```text
av1.eyJ2IjoxLCJqdGkiOiI1ZGU5OTY1ZC0...
kind=leaf jti=5de9965d-... expires_in=60s uses=1 hard_exp=...
```

### 4. 用一次

```bash
agentvault --home "$HOME/.agentvault-demo" resolve --ticket av1...
```

第一次返回密钥值；第二次返回：

```text
error: denied: ticket revoked
```

### 5. 收回一切

```bash
agentvault --home "$HOME/.agentvault-demo" revoke --all
```

`revoke --all` 会推进全局 epoch，所有旧票据立即失效；之后签发的票据使用新 epoch。

---

## Web 控制台

<p align="center">
  <img src="docs/assets/web/console-hero.png" alt="AgentVault local web console" width="100%">
</p>

CLI 解决“配置里不要出现明文”；Web 控制台解决“人怎么在 10 秒内看清现在发生了什么”。它不是另一个服务端产品：`agentvault web` 仍然是一个本地单二进制，React 资源在编译时嵌入二进制，读的是同一个 `AGENTVAULT_HOME`、同一个 broker、同一份审计。

```bash
# 默认 127.0.0.1:8787；启动后会打印访问地址和 web.token 路径
agentvault web

# 换端口
agentvault web --port 8790
```

### 登录方式

| 方式 | 适合谁 | 凭证放在哪 |
|---|---|---|
| **本地会话** | 单机第一次进入、应急管理 | `AGENTVAULT_HOME/web.token`；只在本机、文件权限受限 |
| **本地账号** | 一台机器多人使用、区分 owner / admin / viewer | `web-accounts.json`，密码是 **Argon2id** 哈希；会话 token 只存 SHA-256 哈希 |

<p align="center">
  <img src="docs/assets/web/login.png" alt="Login" width="88%">
</p>

第一次用 `web.token` 以 owner 身份进入，然后在「设置 → 账号与租户」里创建长期账号。默认建议：

- **owner**：只留 1 个，负责创建/删除账号与全局撤销；
- **admin**：日常发凭证、发票据、接入 Agent；
- **viewer**：只读看板与审计，适合值班、审计同事；
- 会话默认 12 小时；viewer 的任何写请求都会被服务端拒绝，而不是只在 UI 隐藏按钮。

> **租户字段是标签，不是硬隔离**：它用于同一台机器上区分团队/用途，方便筛选与交接；v0.1 不承诺跨租户的强隔离或独立密钥空间。

### 控制台能做什么

| 页面 | 关键动作 | 真实截图 |
|---|---|---|
| **概览** | 活跃凭证、活票、24h 签发/拒绝、构成、最近事件、SSE 实时刷新 | 上方 hero |
| **凭证** | 新建/轮换/删除；按 env / token / json 筛选；密文输入 | [secrets.png](docs/assets/web/secrets.png) |
| **票据** | 一次性叶子票、任务父票、续期、撤销、全局吊销 | [tickets.png](docs/assets/web/tickets.png) |
| **接入** | 六个 Agent 适配器、候选路径选择、dry-run 预览、复制 Codex 命令 | [agents.png](docs/assets/web/agents.png) · [agent-picker.png](docs/assets/web/agent-picker.png) |
| **审计** | 事件筛选、复制审计命令、签名/哈希链校验（CLI） | [audit.png](docs/assets/web/audit.png) |
| **蜜罐** | 创建 canary、复制 token、模拟触发、查看命中 | [honeypots.png](docs/assets/web/honeypots.png) |
| **设置** | 后端指纹、票据策略、账号与角色、危险操作 | [settings.png](docs/assets/web/settings.png) · [accounts.png](docs/assets/web/accounts.png) |

<table>
<tr>
<td width="50%"><img src="docs/assets/web/secrets.png" alt="Credentials"><br><sub>凭证：长期值不进配置，页面只显示名称、类型和状态</sub></td>
<td width="50%"><img src="docs/assets/web/tickets.png" alt="Tickets"><br><sub>票据：父票负责“任务还活着”，子票负责“每次调用只活一次”</sub></td>
</tr>
<tr>
<td width="50%"><img src="docs/assets/web/agents.png" alt="Agents"><br><sub>接入：检测到多个候选路径时先让你选，再 dry-run 预览</sub></td>
<td width="50%"><img src="docs/assets/web/audit.png" alt="Audit"><br><sub>审计：追加式 JSONL，签发、解析、撤销、蜜罐都有记录</sub></td>
</tr>
</table>

### 安全边界

- 默认只绑定 `127.0.0.1`；把地址改成非 loopback 需要显式设置 `AGENTVAULT_WEB_BIND`，并且必须先有至少一个账号；
- `web.token` 与 `web-accounts.json`、`web-sessions.json` 都以私有权限写入；控制台不提供“关闭鉴权”的开关；
- 页面不会显示长期密钥值；创建凭证时的输入框是遮罩输入；
- 远程部署不是默认能力：如果确实要跨机器访问，请放在反向代理 + TLS + 网络 ACL 后面，并先读 [安全模型](#安全模型)。不要把裸 HTTP 控制台直接暴露到公网。

---

## 加密备份、签名审计与 TOML 往返

这次升级不只是“加一个页面”，底层也补了三块安全工程。

### 1. 备份不再留下明文

旧行为是 `mcp.json.bak.<timestamp>`，迁移完却把明文密钥留在磁盘上。现在是加密备份：

```bash
# 创建：<config>.bak.<ms>.enc
agentvault backup create --path ~/.codex/config.toml

# 列出加密备份与历史明文备份
agentvault backup list --path ~/.codex/config.toml

# 恢复：先解密到目标，再重新解析校验；失败不会留下半截文件
agentvault backup restore --backup ~/.codex/config.toml.bak.1795350000000.enc --path ~/.codex/config.toml

# 确认新配置可用后，清理备份
agentvault init --refresh --cleanup-backups
```

| 项目 | 实现 |
|---|---|
| 格式 | `AVBAK1` magic + ChaCha20-Poly1305 |
| 备份密钥 | OS keystore 中的 `backup/master`，不落明文 |
| 恢复校验 | JSON / TOML 重新解析通过后才写入 |
| 兼容 | `backup list` 仍能看到旧的 `*.bak.*` 明文文件，方便迁移 |

### 2. 审计不只知道“有没有改”，还知道“是谁签的”

审计仍是追加式 JSONL，但每行都有哈希链，并可用 Ed25519 签名验证：

```bash
agentvault audit --tail 20
agentvault audit --verify                 # 检查哈希链，报告第一处断裂
agentvault audit --verify-signature       # 验证每一行的 Ed25519 签名
agentvault audit --pubkey                 # 打印审计签名公钥（hex）
```

这解决的是“本地日志被悄悄改一行”的问题：哈希链防插入/删除，签名防重写。它不是远程不可篡改日志；要把审计送到只写存储，仍需外部系统。

### 3. Codex TOML 从“行解析”改成完整往返

Codex 的 `config.toml` 不再用最小行解析器，而是 `toml_edit`：

- 单/双引号、转义、多行数组、inline table、dotted key、注释、CRLF、UTF-8 BOM 都保留；
- 写入前重新解析，解析失败就拒绝写入并从备份恢复；
- 非字符串 args / env 不会猜，标记为 `unsupported` 并跳过；
- 真实 Codex 配置副本与多种边界样本进入 corpus 回归。

---

## 图文操作指南

下面按“真实使用顺序”走一遍。每一步都有对应命令、预期输出和容易踩的坑。

### 第 0 步：先看全景

<p align="center">
  <img src="docs/assets/diagrams/architecture.svg" alt="Architecture overview" width="100%">
</p>

你只需要记住一条链路：

```text
init 发现配置 → dry-run 展示计划 → 备份 → 迁移 env → 包装命令 → 校验
```

`init` 不会“看到什么就改什么”。它只处理**计划中要包装的 server**；URL-only、已经包装过、解析失败的 server 会跳过并打印原因。

### 第 1 步：安装并确认环境

```bash
agentvault --version
agentvault doctor
```

示意输出：

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

重点看四项：

- `home`：状态目录在哪里；
- `keystore`：用的是 DPAPI / Keychain / Secret Service / 加密文件；
- `broker`：本地 broker 是否在线；
- 底部 `[x]`：发现了哪些 agent 配置、还有多少 server 待包装。

### 第 2 步：发现 Agent 配置

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

几个细节：

- **项目优先**：在仓库根目录运行时，项目内的 `.cursor/mcp.json`、`.mcp.json` 会排在用户全局配置前面。
- `--agent auto` 只会从存在的候选里选，不会凭空创建配置。
- 想指定具体文件：`agentvault init --path .\mcp.json`。
- 想强制某个客户端：`agentvault init --agent qoder`。

### 第 3 步：dry-run，不改任何文件

这一步是整套工具最重要的安全阀。**先看计划，再决定是否写。**

准备一个测试配置（或直接用你真实的配置副本）：

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

运行：

```bash
agentvault init --path .\mcp.json --dry-run --no-env
```

真实输出：

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

三个桶的含义：

| 输出 | 含义 | 配置里会发生什么 |
|---|---|---|
| `migrate env` | 看起来是密钥 | 值存入 keystore，env 条目被移除，运行时用 `--env KEY=secret:NAME` 注入 |
| `keep env` | 是 `${...}` / `$VAR` / `%VAR%` / `{{...}}` 引用，或被 `--keep-env` 指定 | 原样保留，由 MCP 客户端展开 |
| `unchanged env` | 不像密钥的普通配置 | 原样保留，不进入 keystore |
| `skip` | URL-only、已包装、无法解析 | 完全不动 |

> **注意**：引用检测现在覆盖“嵌入式引用”，例如 `${HOME}/.config`、`$HOME/.config`、`%USERPROFILE%\...`、`prefix-${ENV}-suffix`。之前只识别“整个值就是一个引用”，会把 `${HOME}/.config` 误迁走；这个问题已修复并有单测覆盖。

如果判断错了，两个显式开关可以覆盖：

```bash
# 强制迁移某个 key（即使它看起来像普通配置）
agentvault init --path .\mcp.json --migrate-env SESSION_ID

# 强制保留某个 key（即使它看起来像密钥）
agentvault init --path .\mcp.json --keep-env INTERNAL_HINT
```

### 第 4 步：真正执行 init

确认 dry-run 的计划没问题后：

```bash
agentvault init --path .\mcp.json
```

会发生：

1. 自动确保本地 broker 在线（可 `--no-autostart` 关闭）；
2. 先写一份带时间戳的备份：`mcp.json.bak.<timestamp>`；
3. 只迁移计划中的 env 值到 keystore；
4. 把命令改成 `agentvault run --name postgres -- ...`；
5. 写回配置，并重新解析校验；
6. 打印写了哪个文件、备份在哪。

<p align="center">
  <img src="docs/assets/diagrams/init-flow.svg" alt="init flow" width="100%">
</p>

包装后的 JSON 形态：

```json
{
  "mcpServers": {
    "postgres": {
      "command": "agentvault",
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

几个容易忽略的点：

- 包装后的 `command` 是 **绝对路径**（`launcher_path()`），不是裸 `agentvault`，避免 agent 的 PATH 和你终端不一致。
- 包装会带显式 `--home`，避免不同 shell 的 `AGENTVAULT_HOME` 不一致导致读到另一个 vault。
- 普通 env 保留；只有计划迁移的 key 被移除。
- 二次运行 `init` 是 no-op（幂等），不会重复包装、不会重复迁移。
- 迁移后的值通过 `--env KEY=secret:NAME` 在运行时解析，所以**不需要**改 MCP server 的代码。

### 第 5 步：发出第一张票据

```bash
agentvault issue --secret postgres/DATABASE_URL --ttl 15m --scope read
```

```text
av1.eyJ2IjoxLCJqdGkiOiI...
kind=leaf jti=... expires_in=900s uses=1 hard_exp=...
```

票据是 HMAC-SHA256 签名的 `av1` 字符串，claims 里包含：

- `jti`：票据 ID，审计和撤销都用它；
- `sid`：secret ID；
- `scope`：权限范围（默认 `read`）；
- `iat` / `exp` / `hard_exp`；
- `epoch`（secret 轮换代数）和 `gepoch`（全局撤销代数）；
- `max_uses` / `kind` / `parent` / `renewable`。

你可以用 `--json` 拿结构化输出，方便脚本处理：

```bash
agentvault issue --secret postgres/DATABASE_URL --ttl 15m --json
```

### 第 6 步：一次使用，重放被拒

```bash
TICKET=$(agentvault issue --secret postgres/DATABASE_URL --ttl 60s)
agentvault resolve --ticket "$TICKET"   # 成功，返回值
agentvault resolve --ticket "$TICKET"   # 失败
```

<p align="center">
  <img src="docs/assets/terminal/replay-denied.svg" alt="Replay denied" width="100%">
</p>

真实第二次输出：

```text
error: denied: ticket revoked
```

这不是“客户端 bug”，而是票据系统的基本语义：默认 `max_uses=1`，用掉即撤销。需要多次使用时显式指定：

```bash
agentvault issue --secret postgres/DATABASE_URL --max-uses 3 --ttl 5m
```

### 第 7 步：长任务票据与子票据

一个跑 2 小时的 agent 任务，不应该拿一张 2 小时有效、能反复读取生产密钥的票据。正确姿势：

```bash
# 父票据：短 TTL，可 renew，有 2 小时硬上限
PARENT=$(agentvault issue --secret postgres/DATABASE_URL \
  --task --ttl 30s --hard-ttl 2h --json | jq -r .ticket)

# 每次工具调用前，从父票据派生一张 60 秒叶子票据
CHILD=$(agentvault child --parent "$PARENT" --ttl 60s)
agentvault resolve --ticket "$CHILD"
```

约束：

- 子票据的过期时间 **不能超过** 父票据的 `hard_exp`；
- `renew` 只能把父票据的 `exp` 往后滑，**不能超过 `hard_ttl`**；
- 父票据被撤销，子票据一起失效；
- `run --session` 会自动创建可 renew 的父票据，并在子进程退出时撤销它。

<p align="center">
  <img src="docs/assets/diagrams/ticket-lifecycle.svg" alt="Ticket lifecycle" width="100%">
</p>

### 第 8 步：包装一个真实进程

```bash
agentvault run --name postgres -- npx @modelcontextprotocol/server-postgres
```

或者在一次运行中注入多个已迁移的密钥：

```bash
agentvault run --name postgres \
  --env DATABASE_URL=secret:postgres/DATABASE_URL \
  --env API_KEY=secret:postgres/API_KEY \
  -- npx @modelcontextprotocol/server-postgres
```

行为说明：

- `--name NAME`：如果 keystore 里存在**同名** secret，会导出一张 `AGENTVAULT_TICKET` 给子进程；
- `--env KEY=secret:NAME`：在启动子进程前解析 `NAME`，把值作为普通 env 注入；
- `--session`：创建可 renew 的父票据，并导出：
  - `AGENTVAULT_PARENT_TICKET`
  - `AGENTVAULT_PARENT_FILE`
- 子进程退出后，session 父票据会被撤销；后台 renew 线程停止。

> 为什么 `init` 包装后不自动导出 `AGENTVAULT_TICKET`？因为大多数 MCP server 不认识这个变量，它们只认 `DATABASE_URL` 这类普通 env。所以 `init` 走的是“运行时注入值”的兼容路径；只有需要票据感知的自定义 server，才用直达 `run --name <同名 secret>` 或 `--session`。

### 第 9 步：升级后刷新 launcher

你从 debug 构建换到 release 构建，或者移动了安装目录，不需要重新迁移 env。用 `--refresh`：

```bash
agentvault init --agent codex --refresh --dry-run
agentvault init --agent codex --refresh
```

<p align="center">
  <img src="docs/assets/terminal/refresh-launcher.svg" alt="Refresh launcher" width="100%">
</p>

它只改两件事：

- 已包装 server 的 launcher 路径；
- `--home` 的值。

它**不动**：env、args 中 `run` 之后的原命令、`--no-autostart` 等后缀。JSON 和 Codex TOML 都支持；每个文件仍然先备份。

### 第 10 步：离职、丢失设备与蜜罐

**全局撤销（kill switch）：**

```bash
agentvault revoke --all
```

**撤销一个 secret 的签发权：**

```bash
agentvault revoke --secret postgres/DATABASE_URL
```

**撤销一张票据（jti 或完整 token 都行）：**

```bash
agentvault revoke --ticket 5de9965d-...
agentvault revoke --ticket av1...
```

**轮换：**

```bash
agentvault rotate --secret postgres/DATABASE_URL
```

轮换会生成新值、推进 secret epoch，让旧票据在 resolve 时全部失效；同时清除该 secret 的撤销状态。

**蜜罐：**

```bash
agentvault honeypot add prod/canary --json
# 把返回的 token 放进一个 agent 绝不应该读取的 .env / 配置里
agentvault honeypot check <token>
```

<p align="center">
  <img src="docs/assets/terminal/honeypot-trip.svg" alt="Honeypot trip" width="100%">
</p>

真实输出：

```text
hit=true honeypot=prod/canary revoked_all=true
```

蜜罐命中会触发全局撤销；之后的 resolve 会看到：

```text
error: denied: ticket was invalidated by a global revocation
```

**看审计：**

```bash
agentvault audit --tail 20
```

```text
[1795261677681] ticket.resolve     actor=cli  secret=demo/token  ticket=a705ff08-...  ok
[1795261677724] ticket.resolve     actor=cli  secret=demo/token  ticket=a705ff08-...  revoked
[1795261686286] honeypot.triggered actor=cli  secret=canary      ticket=-             blocked
```

审计是追加式 JSONL；轮换时保留旧文件，不会覆盖历史。

### 第 11 步：跑真实测试

```bash
cargo test --workspace
cargo build --release

# Web 控制台的浏览器级点击测试（需要 Chrome 和 Playwright）
cd webapp && npm run test:e2e
```

本机结果（Windows 11，2026-11-22）：

```text
e2e:        11 passed
adapters:   24 passed
corpus:      4 passed
audit:       2 passed
backup:      1 passed
core:       10 passed
keystore:    2 passed
total:      54 passed, 0 failed, 1 ignored

webapp Playwright: 2 passed (bootstrap token + login → add secret → issue ticket → pick agent path → filter audit)
```

`1 ignored` 是 Windows ConPTY 交互式遮罩输入测试：它需要真实交互终端，CI 和当前非交互 harness 里不伪造通过。

<p align="center">
  <img src="docs/assets/diagrams/test-evidence.svg" alt="Test evidence" width="100%">
</p>

测试覆盖的不是 happy path，而是：

- JSON / Codex / Qoder 配置重写与幂等；
- 完整 TOML 往返：引号、转义、多行数组、inline table、dotted key、注释、CRLF、BOM、未知字段；
- 解析失败时拒绝写入，并用加密/明文备份恢复；
- 一次票据、过期票据、重放拒绝；
- task 票据的 hard-ttl 上限、renew 不缩短；
- revoke token / jti / all、rotate 后旧票据失效；
- 蜜罐触发全局撤销；
- Ed25519 审计签名与哈希链校验；
- ChaCha20-Poly1305 加密备份的创建、恢复、清理；
- Web 控制台的 token 登录、账号角色、viewer 只读、Agent 路径选择、SSE 刷新；
- autostart 开关、单实例、broken pipe；真实 Codex 配置副本的刷新。

> 没有 GUI 级 Cursor/Qoder 工具调用自动化；没有 macOS/Linux 运行时验证；没有 sandbox/egress。这些都在“已知边界”里，不假装有。

---

## 配置识别与 env 迁移

### 路径识别：注册表，不是启发式

- 每个客户端的候选路径是**写死的注册表**，项目路径优先于用户全局路径；
- `init --list` 打印所有候选、存在性、server 数、已包装数、`auto` 会选哪个；
- `--agent` 强制客户端，`--path` 直接指定文件；
- 不递归扫描、不读浏览器历史、不猜“可能在某个目录”。

### env 迁移：启发式，但可见、可覆盖

迁移判断按以下顺序：

1. **引用保护**：`${...}`、`$VAR`、`%VAR%`、`{{...}}`，包括嵌入在路径中的形式（`${HOME}/.config`）→ 永不迁移；
2. **key 标记**：`TOKEN`、`SECRET`、`PASSWORD`、`API_KEY`、`DATABASE_URL`、`DSN`、`WEBHOOK`、`COOKIE` 等；
3. **value 形状**：`sk-`、`ghp_`、JWT、PEM、`postgres://user:pass@...`、长随机串等。

启发式一定会犯错。所以提供三个保险：

```bash
agentvault init --dry-run                 # 先看
agentvault init --migrate-env KEY         # 强制迁移
agentvault init --keep-env KEY            # 强制保留
```

`init --dry-run` 会打印 `migrate env` / `keep env` / `unchanged env` / `skip` 四类；真正的写入永远先备份。

### 备份与失败恢复

- 写入前生成加密备份 `<config>.bak.<ms>.enc`（`AVBAK1` + ChaCha20-Poly1305，密钥在 OS keystore）；
- 写入后重新解析并检查包装结果；失败则从备份恢复；
- `backup list` 仍会列出历史明文 `*.bak.*`，方便迁移与清理；
- `init --refresh --cleanup-backups` 可在确认新配置可用后清理备份。

---

## 命令手册

| 命令 | 作用 | 关键参数 |
|---|---|---|
| `init` | 发现并包装 MCP 配置，迁移密钥 env | `--dry-run` `--list` `--refresh` `--agent` `--path` `--migrate-env` `--keep-env` `--no-env` `--rebind` `--cleanup-backups` |
| `doctor` | 诊断 home、broker、keystore、agent 配置 | `--home` |
| `env show` | 显示 home 来源与持久化值 | `--home` |
| `env install` | 持久化 `AGENTVAULT_HOME` + PATH | `--scope user\|machine` `--no-path` `--dry-run` |
| `env uninstall` | 移除持久化值 | `--scope user\|machine` `--dry-run` |
| `secret add` | 存长期密钥 | `--value` `--stdin` `--kind` `--honeypot` |
| `secret list` | 列出密钥（不显示值） | `--home` |
| `secret remove` | 删除密钥 | `<NAME>` |
| `secret unrevoke` | 允许已撤销名称重新签发（不轮换值） | `<NAME>` |
| `issue` | 签发短期票据 | `--secret` `--ttl` `--scope` `--max-uses` `--task` `--hard-ttl` `--json` |
| `resolve` | 兑换票据（默认精确一次） | `--ticket` `--json` |
| `child` | 从父票据派生叶子票据 | `--parent` / `--parent-file` `--ttl` `--sub` `--max-uses` |
| `renew` | 滑动父票据过期时间，受 hard-ttl 限制 | `--ticket` `--ttl` |
| `run` | 包装进程，注入票据或解析后的值 | `--name` `--env KEY=secret:NAME` `--session` `--ttl` `--hard-ttl` |
| `revoke` | 撤销票据 / secret / 全部 | `--ticket` `--secret` `--all` |
| `rotate` | 轮换值并使旧票据失效 | `--secret` |
| `status` | 查看 secrets、绑定 agent、live tickets | `--json` |
| `audit` | 查看 JSONL 审计 | `--tail N` `--json` `--event` `--since` `--export` `--verify` `--verify-signature` `--pubkey` |
| `honeypot add/list/check` | 管理蜜罐 canary | `--json` |
| `backup create/restore/list` | 加密配置备份、恢复与列表 | `--path` `--backup` |
| `web` | 启动本地 Web 控制台 | `--port` `--dist`；远程绑定需 `AGENTVAULT_WEB_BIND` + `AGENTVAULT_WEB_ALLOW_REMOTE=1` |
| `daemon` | 前台运行 broker | `--detach` |
| `stop` | 停止后台 broker | `--home` |

所有需要 broker 的命令默认会自动拉起一个 detached broker；用 `--no-autostart` 或环境变量 `AGENTVAULT_NO_AUTOSTART=1` 关闭。

---

## 首次运行与 AGENTVAULT_HOME

默认状态目录：

| 平台 | 路径 |
|---|---|
| Windows | `%LOCALAPPDATA%\agentvault` |
| macOS | `~/Library/Application Support/agentvault` |
| Linux | `$XDG_DATA_HOME/agentvault` 或 `~/.local/share/agentvault` |

优先级：`--home` > `AGENTVAULT_HOME` > 平台默认。

第一次真正执行 `init`（使用平台默认目录，而不是 `--home` 覆盖）时，会：

- 把 `AGENTVAULT_HOME` 写到 **user scope**；
- 把二进制所在目录加入 **user PATH**；
- 不静默写 machine-wide 变量；需要时显式 `env install --scope machine`（通常需要提权）。

管理命令：

```bash
agentvault env show
agentvault env install --dry-run
agentvault env install
agentvault env install --scope machine
agentvault env uninstall
agentvault init --no-env   # 首次 init 不碰环境变量
```

注意：

- 已开着的终端不会自动刷新环境；新终端 / 重新登录后生效；
- `--home` 或 `AGENTVAULT_HOME` 覆盖时，首次 init 不会把该临时路径写成全局默认；
- 包装后的 server 会带显式 `--home`，所以即使 agent 的 PATH/环境不同，也会读到同一个 vault。

---

## 真实测试证据

本项目不是“写几个单元测试就说完成”。在 Windows 11 实机上做了：

- **54 项 Rust 测试通过，1 项 ConPTY 交互测试显式 ignored**：`cargo test --workspace`；
- **2 项 Web 浏览器级点击测试**：Playwright 覆盖 token 一次性引导，以及登录、建凭证、发票据、选择 Agent 路径、筛选审计；
- **150 项隔离 CLI 检查**：每次用独立 `--home`，覆盖 init、ticket、revoke、rotate、env、doctor、autostart、broken pipe 等；
- **真实 Codex 配置副本**：复制真实 `config.toml`，加入测试 server，执行 init/refresh，再用 `codex mcp get` 验证 launcher 和 home；真实配置为 5003 字节，操作前做字节级备份与 SHA-256；
- **真实 Cursor / Qoder 环境探测**：确认路径注册表、空配置、项目配置优先级；
- **真实环境变量写入与恢复**：`env install` 用户级写入、幂等、`env uninstall` 恢复；
- **真实攻击语义**：重放被拒、过期被拒、全局撤销、轮换失效、蜜罐触发；
- **新增回归**：完整 TOML 往返、加密备份、Ed25519 审计签名、Web 账号角色与 viewer 只读。

<p align="center">
  <img src="docs/assets/terminal/replay-denied.svg" alt="Replay denied" width="100%">
</p>

<p align="center">
  <img src="docs/assets/terminal/honeypot-trip.svg" alt="Honeypot trip" width="100%">
</p>

---

## 安全模型

<p align="center">
  <img src="docs/assets/diagrams/security-layers.svg" alt="Security model" width="100%">
</p>

### L0：密钥托管

- Windows：DPAPI（通过 PowerShell 调用，不引入额外 native 构建依赖）；
- macOS：Keychain；
- Linux：Secret Service；无桌面/headless 时回退到 0600 加密文件；
- `status` 会打印当前 backend。

### 设备与硬件绑定

- 设备指纹：hostname + user；
- `init --rebind` 可记录硬件指纹（WMI UUID / IOPlatformUUID / DMI product UUID）；
- 指纹不匹配时 broker 拒绝启动；
- 这不是 TPM sealing；TPM 是后续企业版方向。

### 票据

- HMAC-SHA256 签名，密钥由 master key + 全局撤销 epoch 派生；
- `revoke --all` 推进全局 epoch，旧票据一次性全部失效；
- secret 轮换推进 secret epoch，旧票据在 resolve 时被拒；
- 默认 `max_uses=1`；用尽自动撤销；
- task 票据有 `hard_exp`，`renew` 不能越过；
- 子票据的过期时间受父票据 `hard_exp` 限制；
- `revoke --ticket` 接受 jti 或完整 token。

### 本地 broker

- 只监听 `127.0.0.1`；
- 单实例锁；
- `admin.token` 使用 owner-only ACL（Windows `icacls`，Unix 0600）；
- 解密后的 secret 缓存在内存中，避免每次工具调用都拉起 PowerShell；
- broker 退出后，新的需要 broker 的命令会自动拉起。

### 审计与蜜罐

- 追加式 JSONL；记录时间、actor、事件、secret、ticket、结果；
- `audit --tail N` 查看；
- 每行有哈希链，`audit --verify` 报告第一处断裂；
- 每行有 Ed25519 签名，`audit --verify-signature` 逐行验证，`audit --pubkey` 输出公钥；
- 轮换保留旧审计文件；
- 蜜罐 token 是唯一值，命中即触发全局撤销；
- 蜜罐检查不会泄露真实 secret。

### Web 控制台

- 默认绑定 `127.0.0.1`；非 loopback 绑定必须显式开启，且要求至少一个账号；
- 本地会话 token 写在 `web.token`；账号密码 Argon2id，会话 token 只存 SHA-256；
- owner / admin 可写，viewer 在服务端被强制只读；
- 租户字段用于筛选与交接，是逻辑标签，不是强隔离边界；
- 页面不返回长期密钥值；凭证创建使用遮罩输入；
- SSE 只推状态与事件，不推 secret 值。

### 加密备份

- 备份格式 `AVBAK1` + ChaCha20-Poly1305；
- 密钥来自 OS keystore 的 `backup/master`；
- 恢复前重新解析 JSON/TOML，失败不覆盖目标；
- `init --cleanup-backups` 清理加密备份和旧明文 `.bak.*`。

---

## 已知边界

这些是 v0.1 的真实边界，不藏在脚注里：

1. **不是沙箱**：同 OS 用户下的恶意进程仍可读取用户目录、`admin.token`、备份文件。
2. **本地 broker 是本地 TCP**：不是远程服务；能读 `admin.token` 的本地进程可以管理它。
3. **设备绑定不是 TPM**：是指纹检查，不是硬件密封。
4. **不阻止未迁移的明文**：v0.1 只处理它知道的配置；散落在脚本、`.env`、日志里的明文不在范围内。
5. **env 迁移是启发式**：key 标记 + value 形状，会误判；用 dry-run + `--migrate-env` / `--keep-env`。
6. **Codex TOML 以 `toml_edit` 往返**：单/双引号、转义、多行数组、inline table、dotted key、注释、CRLF、BOM 都覆盖；非字符串 args/env 会被标记为 `unsupported` 并跳过，不会猜。
7. **JSON 必须是严格 JSON**：JSONC 注释/尾逗号会被拒绝并报错；UTF-8 BOM 可接受。
8. **`~/.claude.json` 只处理顶层**：`projects.*.mcpServers` 不会自动重写；请对项目 `.mcp.json` 用 `--path`。
9. **`init` 包装默认注入值，不注入票据**：兼容 stock MCP server；票据感知场景用 `run --name <同名 secret>` 或 `--session`。
10. **GUI 工具调用仍未自动化**：Cursor/Qoder 的 GUI 内部工具调用未做自动化验证；CLI、配置层面和 Web 控制台点击路径已验证。
11. **没有 macOS/Linux 运行时验证**：代码和单元测试跨平台，但实机验证目前是 Windows 11。
12. **Web 控制台默认是本地单机模式**：虽然有账号、角色和租户标签，但不要把它当成“已 hardened 的远程多租户服务”；远程绑定需要自己加 TLS、反向代理、网络 ACL 和备份策略。
13. **ConPTY 遮罩输入测试仍为 ignored**：需要真实交互终端，当前测试环境不伪造通过。

---

## 常见问题

### Q1：这和把 token 放进系统环境变量有什么区别？

系统环境变量仍然是长期明文，任何同用户进程都能读，也没有 TTL、次数、撤销、审计。AgentVault 的长期值在 OS keystore，agent 拿到的是短期票据；票据可以过期、用完、撤销。

### Q2：`init` 会把我配置改坏吗？

它先 dry-run 计划、再备份、只改计划中的 server、写后重新解析校验，失败自动恢复。未知字段会保留。即便如此，生产配置仍建议先复制一份，或先在 `--path` 指向的副本上试。

### Q3：为什么我的 `${HOME}/.config` 没有被迁移？

这是有意的。它是引用，不是密钥；迁移会把引用变成字面量，破坏 MCP 客户端的展开。`PASS_THROUGH` 这类引用会出现在 `keep env`。

### Q4：为什么 `init` 后 agent 拿到的是值，不是票据？

因为绝大多数 MCP server 只认 `DATABASE_URL`、`API_KEY` 这类普通 env。`init` 选择兼容路径：启动时用票据换值，再把值注入 env。需要票据感知的自定义 server，用 `run --name <同名 secret>` 或 `--session`。

### Q5：`revoke --all` 之后还能继续用吗？

可以。全局撤销推进 epoch；之后新签发的票据使用新 epoch，正常可用。旧票据全部失效。

### Q6：轮换和撤销有什么区别？

- `revoke --secret`：阻止该 secret 继续签发；已有票据也失效；值还在。
- `rotate --secret`：生成新值，推进 epoch，旧票据失效，并清除该 secret 的撤销状态。
- `revoke --all`：所有 secret 的所有旧票据一次性失效。

### Q7：broker 会不会拖慢每次工具调用？

broker 在内存中缓存解密后的 secret；DPAPI 只在启动时调用一次。常规 issue/resolve 是本地 TCP + HMAC 校验，开销很小。

### Q8：我能把它接到自己的 MCP server 吗？

可以。两种方式：

```bash
# 方式 1：普通 env 注入（兼容 stock server）
agentvault run --name my-server --env API_KEY=secret:my/API_KEY -- node server.js

# 方式 2：票据感知
agentvault run --name my/API_KEY -- node server.js
# 子进程读取 AGENTVAULT_TICKET，再调用 agentvault resolve
```

### Q9：Web 控制台和 CLI 是什么关系？

CLI 仍然是最小可信面，也是自动化入口；Web 控制台是同一个二进制上的本地可视化层，读同一个 `AGENTVAULT_HOME` 和 broker。默认只监听 `127.0.0.1`，第一次用 `web.token` 登录，之后可以在设置里创建 owner/admin/viewer 账号。它不是云端 SaaS，也不默认开放远程访问。

### Q10：支持团队共享一个 vault 吗？

v0.1 支持一台机器上的 owner/admin/viewer 账号和租户标签，适合小团队共用一台工作站；它**不**支持把同一个 home 放到网络盘上做并发共享，也不承诺跨机器的强隔离。团队集中策略、远程审计和真正的多租户是后续方向。

---

## 项目结构

```text
crates/
  core/      domain types, token signing, paths, JSON store
  keystore/  OS keystore + encrypted-file fallback + derivation
  audit/     append-only JSONL audit + hash chain + Ed25519 signatures
  backup/    AVBAK1 encrypted config backups (ChaCha20-Poly1305)
  adapters/  JSON mcpServers + full Codex TOML round-trip via toml_edit
  broker/    local TCP broker, ticket lifecycle, client
  web/       axum console, auth/roles/tenants, SSE, embedded assets
  cli/       agentvault binary
webapp/      React + Vite console UI, Playwright click test
docs/
  assets/    logo, diagrams, terminal evidence, web screenshots
```

关键入口：

- `crates/cli/src/main.rs`：命令解析、init/issue/run/doctor/web 等；
- `crates/adapters/src/lib.rs`：路径注册表、JSON 计划/应用/刷新、env 启发式；
- `crates/adapters/src/codex.rs`：Codex TOML 完整往返与失败关闸；
- `crates/backup/src/lib.rs`：加密备份与恢复；
- `crates/audit/src/lib.rs`：哈希链与 Ed25519 签名；
- `crates/broker/src/lib.rs`：本地 broker 与票据生命周期；
- `crates/core/src/token.rs`：HMAC 票据签发/校验；
- `crates/keystore/src/lib.rs`：DPAPI / Keychain / Secret Service / file backend；
- `crates/web/src/lib.rs`、`webapp/src/App.tsx`：控制台 API 与界面。

---

## 路线图

- [x] L0：OS keystore、设备绑定、硬件指纹、票据签名
- [x] L1：本地 broker、短期票据、task/child/renew、rotate/revoke
- [x] L4：JSONL 审计、Ed25519 签名、蜜罐 canary、全局撤销
- [x] `init --refresh`：升级后无痛换 launcher/home
- [x] 嵌入式 env 引用保护（`${HOME}/...`、`%VAR%\...`）
- [x] Web 控制台：单二进制、账号/角色/租户标签、SSE、路径选择器
- [x] 加密备份（AVBAK1）、完整 TOML 往返、审计签名校验
- [ ] 桌面壳：本地预览已完成，暂不随仓库提交
- [ ] L2：进程沙箱 / microVM
- [ ] L3：egress allowlist / DLP
- [ ] Web 远程部署加固 / 集中策略 / 多租户强隔离
- [ ] macOS/Linux 实机验证与 CI 覆盖
- [ ] TPM / Secure Enclave 绑定
- [ ] 团队策略与集中审计

---

## 贡献与安全

- 贡献指南：[CONTRIBUTING.md](CONTRIBUTING.md)
- 安全策略与漏洞报告：[SECURITY.md](SECURITY.md)
- 不要在 issue / PR 中粘贴真实 token、cookie、生产配置；用 `sk-test-...` 合成值。
- 配置重写必须保留备份；不要提交 `*.bak.*`。
- 文档以中文 README 为准，行为变化时同步更新 EN/JA/KO。

---

## 许可证

Apache License 2.0。见 [LICENSE](LICENSE)。

---

<p align="center">
  <sub>Built for agents that should borrow credentials, not own them.</sub>
</p>

<p align="center">
  <a href="https://star-history.com/#Seramys/agentvault&Date">
    <img src="https://api.star-history.com/svg?repos=Seramys/agentvault&type=Date" alt="Star history" width="720">
  </a>
</p>