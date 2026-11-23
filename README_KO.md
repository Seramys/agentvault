<p align="center">
  <img src="docs/assets/brand/logo.png" width="150" alt="AgentVault logo">
</p>

<h1 align="center">AgentVault</h1>

<p align="center">
  <strong>AI 에이전트의 장기 자격 증명을 일회용 티켓으로 바꿉니다.</strong><br>
  장기 시크릿은 OS 키스토어에 남기고, 에이전트에는 만료·폐기 가능하며 기본 1회용인 티켓을 전달합니다.
</p>

<p align="center">
  <a href="https://github.com/Seramys/agentvault/actions/workflows/ci.yml"><img src="https://github.com/Seramys/agentvault/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-6D5BD0.svg" alt="License"></a>
  <img src="https://img.shields.io/badge/rust-stable-CE422B.svg" alt="Rust stable">
  <img src="https://img.shields.io/badge/status-v0.1%20%C2%B7%20web-0F9F6E.svg" alt="v0.1 · web">
</p>

<p align="center">
  <a href="README.md">简体中文</a> · <a href="README_EN.md">English</a> · <a href="README_JA.md">日本語</a> · <b>한국어</b>
</p>

<p align="center">
  <a href="#30초-이해">30초</a> ·
  <a href="#빠른-시작">빠른 시작</a> ·
  <a href="#web-콘솔">Web 콘솔</a> ·
  <a href="#단계별-가이드">가이드</a> ·
  <a href="#명령어-목록">명령어</a> ·
  <a href="#보안-모델">보안</a> ·
  <a href="#실제-테스트-증거">증거</a>
</p>

> **현재 범위**: 레이어 0(시크릿 보관), 1(단기 티켓 + 로컬 broker), 4(감사 + 허니팟), 그리고 로컬 Web 콘솔(계정/역할, 테넌트 라벨, SSE, 서명 감사, 암호화 백업)을 구현했습니다. 샌드박스(L2)와 이그레스 제어/DLP(L3)는 향후 작업입니다.
>
> **한 문장**: token을 `mcp.json`에 넣을지 고민하지 마세요. Vault에 보관하고 에이전트가 티켓으로 교환하게 하세요. 전체 그림이 필요하면 로컬 콘솔을 여세요.

---

## 목차

<details>
<summary>펼치기</summary>

- [해결하는 문제](#해결하는-문제)
- [30초 이해](#30초-이해)
- [이것은 무엇이고, 무엇이 아닌가](#이것은-무엇이고-무엇이-아닌가)
- [지원 에이전트](#지원-에이전트)
- [5개 레이어](#5개-레이어)
- [설치](#설치)
- [빠른 시작](#빠른-시작)
- [Web 콘솔](#web-콘솔)
- [암호화 백업, 서명 감사, TOML 왕복](#암호화-백업-서명-감사-toml-왕복)
- [단계별 가이드](#단계별-가이드)
  - [0단계: 전체 그림](#0단계-전체-그림)
  - [1단계: 설치 및 확인](#1단계-설치-및-확인)
  - [2단계: 에이전트 설정 탐색](#2단계-에이전트-설정-탐색)
  - [3단계: dry-run으로 아무것도 바꾸지 않기](#3단계-dry-run으로-아무것도-바꾸지-않기)
  - [4단계: 실제 init 실행](#4단계-실제-init-실행)
  - [5단계: 첫 티켓 발급](#5단계-첫-티켓-발급)
  - [6단계: 1회 사용, 재생 거부](#6단계-1회-사용-재생-거부)
  - [7단계: 작업 티켓과 자식 티켓](#7단계-작업-티켓과-자식-티켓)
  - [8단계: 실제 프로세스 래핑](#8단계-실제-프로세스-래핑)
  - [9단계: 업그레이드 후 launcher 새로고침](#9단계-업그레이드-후-launcher-새로고침)
  - [10단계: 퇴사, 분실, 허니팟](#10단계-퇴사-분실-허니팟)
  - [11단계: 실제 테스트 실행](#11단계-실제-테스트-실행)
- [설정 탐지와 env 마이그레이션](#설정-탐지와-env-마이그레이션)
- [명령어 목록](#명령어-목록)
- [첫 실행과 AGENTVAULT_HOME](#첫-실행과-agentvault_home)
- [실제 테스트 증거](#실제-테스트-증거)
- [보안 모델](#보안-모델)
- [알려진 한계](#알려진-한계)
- [FAQ](#faq)
- [저장소 구조](#저장소-구조)
- [로드맵](#로드맵)
- [기여 및 보안](#기여-및-보안)
- [라이선스](#라이선스)

</details>

---

## 해결하는 문제

Claude Code, Cursor, Codex, Qoder 같은 MCP 클라이언트를 데이터베이스, GitHub, 브라우저, 내부 API에 연결할 때 흔한 방식은 설정에 시크릿을 직접 쓰는 것입니다.

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

여기서 세 가지 현실적인 문제가 생깁니다.

1. **설정 파일이 평문 시크릿 저장소가 됩니다.** 복사, 스크린샷, git commit으로 시크릿이 함께 이동합니다.
2. **권한에 시간 경계가 없습니다.** 에이전트가 값을 얻으면 영구적으로 사용할 수 있습니다. "15분만 읽기"는 사람의 절제에 의존합니다.
3. **사고 시 답할 수 없습니다.** 누가, 언제, 어떤 키를 썼는가? 감사도, 폐기도, 일회용 의미도 없습니다.

AgentVault는 흐름을 바꿉니다.

```text
장기 시크릿  ──►  OS 키스토어  ──►  단기 티켓(기본 1회)  ──►  에이전트 프로세스
     ▲                                    │
     └──────── 감사 / 폐기 / 로테이션 ◄────┘
```

설정에 평문이 남지 않습니다. 에이전트는 `av1...` 티켓을 받습니다. 만료, 사용 완료, 폐기, 전역 kill switch 중 하나라도 발생하면 resolve가 거부됩니다.

---

## 30초 이해

<p align="center">
  <img src="docs/assets/diagrams/architecture.svg" alt="AgentVault architecture" width="100%">
</p>

세 가지 역할:

| 역할 | 하는 일 | 하지 않는 일 |
|---|---|---|
| **Vault** | 장기 시크릿을 OS 키스토어에 보관하고 티켓을 서명/검증 | 장기 시크릿을 에이전트에 전달 |
| **Broker** | `127.0.0.1`에서만 수신하고 issue/resolve/revoke/rotate 처리 | 네트워크에 노출, 원격 서비스화 |
| **Agent wrapper** | `agentvault run`으로 MCP server를 시작하고 티켓 또는 단기 값을 주입 | 래핑 대상 MCP 프로토콜 변경 |

가장 짧은 체험:

```bash
# 1. 장기 시크릿 저장 (설정에는 들어가지 않음)
agentvault secret add demo/token --value 'sk-test-demo-123' --kind env

# 2. 60초·1회용 티켓 발급
agentvault issue --secret demo/token --ttl 60s

# 3. 첫 resolve는 성공, 두 번째는 거부
agentvault resolve --ticket av1...
```

---

## 이것은 무엇이고, 무엇이 아닌가

**이것은:**

- 로컬 CLI + 로컬 broker 기반의 **자격 증명 수명주기 레이어**입니다.
- MCP 설정의 평문 env를 **OS 키스토어**로 옮기고 명령을 `agentvault run ...`으로 다시 쓰는 마이그레이션 도구입니다.
- **TTL, max_uses, 작업 티켓, 자식 티켓, renew, rotate, revoke**를 갖춘 티켓 시스템입니다.
- **JSONL 감사, 허니팟 canary, 전역 폐기**를 갖춘 로컬 관측 레이어입니다.

**이것은 아닙니다:**

- 샌드박스가 아닙니다. 같은 OS 사용자 권한의 악성 프로세스가 파일을 읽는 것을 막지 못합니다.
- 네트워크 프록시가 아닙니다. 이그레스 제어나 DLP를 제공하지 않습니다.
- 클라우드 KMS가 아닙니다. 계정 등록도, 시크릿 업로드도 없습니다.
- 제로 트러스트의 은탄환이 아닙니다. 로컬에서 실용적으로 운영 가능하고 감사·폐기 가능한 중간 레이어입니다.

---

## 지원 에이전트

경로 탐지는 추측이 아니라 **클라이언트별 정적 레지스트리 + 프로젝트 우선**입니다. 모든 후보는 `init --list`와 `doctor`에 표시되며, 존재 여부, server 수, 이미 래핑된 수를 알 수 있습니다.

| 에이전트 | 후보 설정 | 쓰기 형식 |
|---|---|---|
| **Claude Code** | project `.mcp.json` → `~/.claude.json` → `~/.claude/mcp.json` | JSON `mcpServers` |
| **Cursor** | project `.cursor/mcp.json` → `~/.cursor/mcp.json` | JSON `mcpServers` |
| **Codex** | `~/.codex/config.toml` | TOML (inline env, env 하위 테이블, 여러 줄 args) |
| **Gemini CLI** | project `.gemini/settings.json` → `~/.gemini/settings.json` | JSON `mcpServers` |
| **Windsurf** | project `.windsurf/mcp.json` → `~/.codeium/windsurf/mcp_config.json` | JSON `mcpServers` |
| **Cline** | VS Code / Cursor / Insiders `globalStorage/.../cline_mcp_settings.json` | JSON `mcpServers` |
| **Qoder** | project `.qoder/settings.json` → `.qoder/settings.local.json` → `.mcp.json` → `~/.qoder/settings.json` | JSON `mcpServers` |
| **기타** | `--path`로 지정한 모든 `mcpServers` JSON | JSON `mcpServers` |

> 전체 디스크 재귀 스캔은 하지 않습니다. 레지스트리 후보 + 명시적 `--path`만 사용합니다. 동작이 예측 가능하고 관련 없는 파일을 건드리지 않습니다.

알 수 없는 필드(`type`, `cwd`, `disabled`, `headers`, `timeout`, `trust`, `autoApprove` 등)는 재작성 시 보존됩니다.

---

## 5개 레이어

<p align="center">
  <img src="docs/assets/diagrams/security-layers.svg" alt="Security layers" width="100%">
</p>

| 레이어 | 내용 | v0.1 |
|---|---|---|
| **L0 시크릿 보관** | OS 키스토어, 장치/사용자 + 하드웨어 지문, 서명된 티켓 | ✅ 구현 |
| **L1 티켓 + broker** | 명령 래핑, 로컬 broker, 단기 티켓, 작업 티켓, renew, rotate, revoke | ✅ 구현 |
| **L2 샌드박스** | 프로세스 격리 / microVM | ⏳ 이후 |
| **L3 이그레스 제어** | allowlist / DLP | ⏳ 이후 |
| **L4 감사 + 허니팟** |추가형 JSONL, canary, 전역 폐기 | ✅ 구현 |

L0 + L1은 "설정에 평문을 두지 않기", "권한에 시간 경계 두기", "사고 시 폐기하기"를 해결합니다. L2/L3는 더 큰 프로젝트이며 완료된 척하지 않습니다.

---

## 설치

### 방법 A: 소스 빌드 (현재 권장)

stable Rust(edition 2021)가 필요합니다. Windows / macOS / Linux에서 동작합니다. Web 콘솔을 포함하려면 Node.js 20+로 프론트엔드를 한 번 빌드합니다(CLI만이면 생략 가능).

```bash
git clone https://github.com/Seramys/agentvault.git
cd agentvault

# Web 콘솔 포함: 컴파일 시점에 내장
cd webapp && npm ci && npm run build && cd ..

cargo build --release
```

산출물:

- Windows: `target\release\agentvault.exe`
- macOS / Linux: `target/release/agentvault`

안정적인 디렉터리(예: `~/.local/bin`, `C:\Tools\agentvault`)에 두고 사용자 환경 변수를 영구화합니다.

```bash
agentvault env install --dry-run   # 변경 내용 확인
agentvault env install             # 사용자 범위 AGENTVAULT_HOME + PATH
```

> 새 터미널 / 새 로그인 세션에서만 반영됩니다. 이미 열린 shell은 재시작하세요.

### 방법 B: `cargo install`

```bash
cargo install --path crates/cli
agentvault --version
```

### 방법 C: GitHub Releases

Releases 페이지에는 v0.1.0 CLI 바이너리가 있습니다. Web 콘솔, 암호화 백업, 서명 감사 같은 새 기능은 다음 릴리스 전까지 소스에서 빌드하세요.

---

## 빠른 시작

> 아래 모든 예시는 `--home`으로 격리 디렉터리를 지정합니다. 먼저 안전하게 시험하고, 확인 후 `--home`을 제거하세요.

### 1. 장기 시크릿 저장

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

### 2. 확인

```bash
agentvault --home "$HOME/.agentvault-demo" secret list
```

```text
demo/token                   kind=env    epoch=0   honeypot=false revoked=false
```

### 3. 티켓 발급

```bash
agentvault --home "$HOME/.agentvault-demo" issue --secret demo/token --ttl 60s
```

```text
av1.eyJ2IjoxLCJqdGkiOiI1ZGU5OTY1ZC0...
kind=leaf jti=5de9965d-... expires_in=60s uses=1 hard_exp=...
```

### 4. 한 번 사용

```bash
agentvault --home "$HOME/.agentvault-demo" resolve --ticket av1...
```

첫 호출은 값을 반환하고, 두 번째는 다음을 반환합니다.

```text
error: denied: ticket revoked
```

### 5. 모두 폐기

```bash
agentvault --home "$HOME/.agentvault-demo" revoke --all
```

`revoke --all`은 전역 epoch를 올립니다. 모든 이전 티켓이 즉시 무효화되고, 이후 발급된 티켓은 새 epoch를 사용합니다.

UI로 시작하려면:

```bash
agentvault web --port 8787
# http://127.0.0.1:8787 열기
# 첫 로그인은 AGENTVAULT_HOME/web.token
```

---

## Web 콘솔

<p align="center">
  <img src="docs/assets/web/console-hero.png" alt="AgentVault local web console" width="100%">
</p>

CLI는 설정에서 평문을 제거합니다. Web 콘솔은 "지금 무슨 일이 일어나는가"를 10초 안에 파악하기 위한 로컬 시각화 계층입니다. 별도 서버 제품이 아닙니다. `agentvault web`은 여전히 단일 바이너리이며, React 빌드를 컴파일 시점에 내장하고 같은 `AGENTVAULT_HOME`, 같은 broker, 같은 감사를 읽습니다.

```bash
# 기본 127.0.0.1:8787. URL과 web.token 경로를 출력
agentvault web

# 포트 변경
agentvault web --port 8790
```

### 로그인

| 방식 | 적합한 용도 | 자격 증명 위치 |
|---|---|---|
| **로컬 세션** | 단일 머신 첫 진입, 비상 관리 | `AGENTVAULT_HOME/web.token`(소유자 전용 파일) |
| **로컬 계정** | 한 머신을 여러 명이 사용, owner/admin/viewer 분리 | `web-accounts.json`에 **Argon2id**, 세션은 SHA-256 저장 |

<p align="center">
  <img src="docs/assets/web/login.png" alt="Login" width="88%">
</p>

처음에는 `web.token`으로 owner 로그인한 뒤 **설정 → 계정과 테넌트**에서 장기 계정을 만듭니다. 권장 구성:

- **owner**: 1개만. 계정 생성/삭제와 전역 폐기 담당;
- **admin**: 일상 시크릿, 티켓, Agent 연결;
- **viewer**: 대시보드와 감사 읽기 전용. 당직/감사용;
- 세션은 12시간. viewer 쓰기는 UI에서 숨기는 것뿐 아니라 서버에서 거부합니다.

> **테넌트는 라벨이며 강한 격리 경계가 아닙니다.** 같은 머신에서 팀/용도를 구분하기 위한 필터입니다.

### 할 수 있는 일

| 페이지 | 주요 작업 | 스크린샷 |
|---|---|---|
| **개요** | 활성 시크릿, 라이브 티켓, 24h 발급/거부, 구성, 최근 이벤트, SSE | 위 hero |
| **시크릿** | 추가/회전/삭제, env/token/json 필터, 마스크 입력 | [secrets.png](docs/assets/web/secrets.png) |
| **티켓** | 1회용 리프, 작업 부모, 갱신, 폐기, 전역 kill switch; 시크릿은 직접 그린 메뉴로 선택하고 TTL은 직접 입력 | [tickets.png](docs/assets/web/tickets.png) · [issue.png](docs/assets/web/issue.png) |
| **연결** | 6개 어댑터, 후보 경로 선택, dry-run 미리보기, Codex 명령 복사 | [agents.png](docs/assets/web/agents.png) · [agent-picker.png](docs/assets/web/agent-picker.png) |
| **감사** | 이벤트 필터, 감사 명령, 해시 체인/서명 검증(CLI) | [audit.png](docs/assets/web/audit.png) |
| **허니팟** | canary 생성, token 복사, 모의 발동, 히트 확인 | [honeypots.png](docs/assets/web/honeypots.png) |
| **설정** | 백엔드 지문, 티켓 정책, 계정/역할, 위험 작업 | [settings.png](docs/assets/web/settings.png) · [accounts.png](docs/assets/web/accounts.png) |

<table>
<tr>
<td width="50%"><img src="docs/assets/web/secrets.png" alt="Credentials"><br><sub>시크릿: 이름·유형·상태만. 장기 값은 페이지에 나오지 않습니다</sub></td>
<td width="50%"><img src="docs/assets/web/tickets.png" alt="Tickets"><br><sub>티켓: 부모는 작업을 살리고, 리프는 정확히 한 번 사용됩니다</sub></td>
</tr>
<tr>
<td width="50%"><img src="docs/assets/web/agents.png" alt="Agents"><br><sub>연결: 후보가 여러 개면 먼저 선택하고 dry-run으로 확인</sub></td>
<td width="50%"><img src="docs/assets/web/audit.png" alt="Audit"><br><sub>감사: 발급·해석·폐기·허니팟을 추가형 JSONL로 기록</sub></td>
</tr>
<tr>
<td width="50%"><img src="docs/assets/web/issue.png" alt="Issue ticket"><br><sub>티켓 발급: 시크릿은 직접 그린 메뉴를 사용하며 브라우저 기본 select가 아닙니다. TTL은 직접 입력합니다. 목록은 현재 home의 실제 데이터입니다</sub></td>
<td width="50%"><img src="docs/assets/web/agent-picker.png" alt="Agent path picker"><br><sub>경로 선택기도 같은 메뉴 컴포넌트로, 키보드·Esc·바깥 클릭을 지원합니다</sub></td>
</tr>
</table>

> 스크린샷의 `github/readonly` 같은 이름은 데모 home의 시드 데이터입니다. 실행 시에는 `/api/secrets`에서 가져오며, 현재 home에 있는 시크릿만 표시하고 하드코딩된 예시 값은 없습니다.

### 보안 경계

- 기본은 `127.0.0.1`만. 비 loopback은 `AGENTVAULT_WEB_BIND`를 명시하고 최소 1개 계정이 필요합니다.
- `web.token`, `web-accounts.json`, `web-sessions.json`은 소유자 전용 권한으로 기록되며, 인증 비활성화 스위치는 없습니다.
- 장기 시크릿 값을 UI로 반환하지 않습니다. 생성 입력은 마스크 처리됩니다.
- 원격 노출은 기본 기능이 아닙니다. 필요하면 리버스 프록시 + TLS + 네트워크 ACL 뒤에 두고, 평문 HTTP를 인터넷에 노출하지 마세요.

---

## 암호화 백업, 서명 감사, TOML 왕복

### 1. 백업에 평문을 남기지 않기

```bash
agentvault backup create --path ~/.codex/config.toml
agentvault backup list --path ~/.codex/config.toml
agentvault backup restore --backup ~/.codex/config.toml.bak.1795350000000.enc --path ~/.codex/config.toml
agentvault init --refresh --cleanup-backups
```

| 항목 | 구현 |
|---|---|
| 형식 | `AVBAK1` + ChaCha20-Poly1305 |
| 키 | OS 키스토어의 `backup/master` |
| 복구 | JSON/TOML 재파싱 후 덮어쓰기 |
| 호환 | `backup list`는 기존 평문 `*.bak.*`도 표시 |

### 2. 해시 체인 + Ed25519 서명

```bash
agentvault audit --tail 20
agentvault audit --verify
agentvault audit --verify-signature
agentvault audit --pubkey
```

해시 체인은 삽입/삭제를, Ed25519 서명은 재작성을 감지합니다. 로컬 변조 증거이며 원격 불변 로그는 아닙니다.

### 3. Codex TOML은 `toml_edit`로 왕복

따옴표, 이스케이프, 여러 줄 배열, inline table, dotted key, 주석, CRLF, BOM을 보존합니다. 잘못된 TOML은 쓰기 전에 거부하고, 문자열이 아닌 args/env는 `unsupported`로 건너뜁니다. 실제 Codex 설정 복사본과 경계 샘플을 회귀 테스트에 포함했습니다.

---

## 단계별 가이드

실제 사용 순서를 따릅니다. 각 단계에 명령, 예상 출력, 주의점이 있습니다.

### 0단계: 전체 그림

<p align="center">
  <img src="docs/assets/diagrams/architecture.svg" alt="Architecture overview" width="100%">
</p>

하나의 파이프라인:

```text
init이 설정 탐지 → dry-run으로 계획 표시 → 백업 → env 마이그레이션 → 명령 래핑 → 검증
```

`init`은 "찾은 것을 무조건 변경"하지 않습니다. 래핑 계획에 포함된 server만 건드리며, URL-only, 이미 래핑됨, 파싱 불가는 이유와 함께 건너뜁니다.

### 1단계: 설치 및 확인

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

확인할 네 가지:

- `home`: 상태 디렉터리;
- `keystore`: DPAPI / Keychain / Secret Service / 암호화 파일;
- `broker`: 로컬 broker 온라인 여부;
- 하단 `[x]`: 발견된 에이전트 설정과 남은 래핑 수.

### 2단계: 에이전트 설정 탐색

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

포인트:

- **프로젝트 우선**: 저장소 루트에서 실행하면 프로젝트의 `.cursor/mcp.json`, `.mcp.json`이 사용자 전역보다 먼저 선택됩니다.
- `--agent auto`는 존재하는 후보 중에서만 선택하며 파일을 생성하지 않습니다.
- 특정 파일: `agentvault init --path .\mcp.json`.
- 클라이언트 강제: `agentvault init --agent qoder`.

### 3단계: dry-run으로 아무것도 바꾸지 않기

가장 중요한 안전장치입니다. **계획을 읽고 나서 씁니다.**

테스트 설정(또는 실제 설정 복사본)을 준비합니다.

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

실행:

```bash
agentvault init --path .\mcp.json --dry-run --no-env
```

실제 출력:

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

네 가지 버킷:

| 출력 | 의미 | 설정에 미치는 영향 |
|---|---|---|
| `migrate env` | 시크릿처럼 보임 | 값을 키스토어로 이동, env 항목 제거, 런타임에 `--env KEY=secret:NAME`으로 주입 |
| `keep env` | `${...}` / `$VAR` / `%VAR%` / `{{...}}` 참조 또는 `--keep-env` 지정 | 그대로 유지, MCP 클라이언트가 확장 |
| `unchanged env` | 일반 설정 | 그대로 유지, 저장하지 않음 |
| `skip` | URL-only, 이미 래핑됨, 파싱 불가 | 전혀 건드리지 않음 |

> **참고**: 참조 탐지는 이제 내장 참조도 포함합니다. `${HOME}/.config`, `$HOME/.config`, `%USERPROFILE%\...`, `prefix-${ENV}-suffix` 등입니다. 이전에는 "값 전체가 참조"인 경우만 인식해 `${HOME}/.config`를 잘못 마이그레이션했습니다. 수정되었고 단위 테스트가 있습니다.

잘못 판단했다면 명시적으로 덮어쓸 수 있습니다.

```bash
# 평범해 보여도 강제 마이그레이션
agentvault init --path .\mcp.json --migrate-env SESSION_ID

# 시크릿처럼 보여도 강제 유지
agentvault init --path .\mcp.json --keep-env INTERNAL_HINT
```

### 4단계: 실제 init 실행

dry-run 계획이 맞다면:

```bash
agentvault init --path .\mcp.json
```

일어나는 일:

1. 로컬 broker 확보(`--no-autostart`로 비활성화);
2. 타임스탬프 백업 `mcp.json.bak.<timestamp>` 작성;
3. 계획된 env 값만 키스토어로 마이그레이션;
4. 명령을 `agentvault run --name postgres -- ...`로 재작성;
5. 설정을 다시 쓰고 재파싱으로 검증;
6. 쓴 파일과 백업 경로 출력.

<p align="center">
  <img src="docs/assets/diagrams/init-flow.svg" alt="init flow" width="100%">
</p>

래핑된 JSON 형태:

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

알아둘 점:

- 래핑된 `command`는 **절대 경로**(`launcher_path()`)이며 맨 `agentvault`가 아닙니다. 에이전트 PATH에 의존하지 않습니다.
- 래퍼는 명시적 `--home`을 가져 shell마다 다른 vault를 읽는 사고를 막습니다.
- 일반 env는 보존되고, 계획된 key만 제거됩니다.
- 두 번째 `init`은 no-op(멱등)입니다. 이중 래핑이나 재마이그레이션이 없습니다.
- 마이그레이션 값은 런타임에 `--env KEY=secret:NAME`으로 해석되므로 MCP server 코드 변경이 필요 없습니다.

### 5단계: 첫 티켓 발급

```bash
agentvault issue --secret postgres/DATABASE_URL --ttl 15m --scope read
```

```text
av1.eyJ2IjoxLCJqdGkiOiI...
kind=leaf jti=... expires_in=900s uses=1 hard_exp=...
```

`av1` 티켓은 HMAC-SHA256으로 서명됩니다. claims에는 다음이 포함됩니다.

- `jti`: 감사와 폐기에 쓰는 티켓 ID;
- `sid`: secret ID;
- `scope`: 권한 범위(기본 `read`);
- `iat` / `exp` / `hard_exp`;
- `epoch`(secret 로테이션 세대)와 `gepoch`(전역 폐기 세대);
- `max_uses` / `kind` / `parent` / `renewable`.

스크립트용 `--json`:

```bash
agentvault issue --secret postgres/DATABASE_URL --ttl 15m --json
```

### 6단계: 1회 사용, 재생 거부

```bash
TICKET=$(agentvault issue --secret postgres/DATABASE_URL --ttl 60s)
agentvault resolve --ticket "$TICKET"   # 성공, 값 반환
agentvault resolve --ticket "$TICKET"   # 거부
```

<p align="center">
  <img src="docs/assets/terminal/replay-denied.svg" alt="Replay denied" width="100%">
</p>

실제 두 번째 출력:

```text
error: denied: ticket revoked
```

클라이언트 버그가 아니라 설계입니다. 기본값은 `max_uses=1`이며 사용하면 폐기됩니다. 여러 번 필요하면 명시하세요.

```bash
agentvault issue --secret postgres/DATABASE_URL --max-uses 3 --ttl 5m
```

### 7단계: 작업 티켓과 자식 티켓

2시간짜리 에이전트 작업에 2시간 유효하고 재사용 가능한 프로덕션 키를 줘서는 안 됩니다.

```bash
# 부모: 짧은 TTL, renew 가능, 2시간 hard cap
PARENT=$(agentvault issue --secret postgres/DATABASE_URL \
  --task --ttl 30s --hard-ttl 2h --json | jq -r .ticket)

# 각 도구 호출 전에 60초 리프 티켓 발급
CHILD=$(agentvault child --parent "$PARENT" --ttl 60s)
agentvault resolve --ticket "$CHILD"
```

규칙:

- 자식 만료는 부모 `hard_exp`를 넘을 수 없습니다.
- `renew`는 부모 `exp`를 뒤로 밀지만 `hard_ttl`을 넘을 수 없습니다.
- 부모를 폐기하면 자식도 무효화됩니다.
- `run --session`은 renew 가능한 부모를 만들고 자식 프로세스 종료 시 폐기합니다.

<p align="center">
  <img src="docs/assets/diagrams/ticket-lifecycle.svg" alt="Ticket lifecycle" width="100%">
</p>

### 8단계: 실제 프로세스 래핑

```bash
agentvault run --name postgres -- npx @modelcontextprotocol/server-postgres
```

여러 마이그레이션 시크릿을 한 번에 주입:

```bash
agentvault run --name postgres \
  --env DATABASE_URL=secret:postgres/DATABASE_URL \
  --env API_KEY=secret:postgres/API_KEY \
  -- npx @modelcontextprotocol/server-postgres
```

동작:

- `--name NAME`: **정확히 같은 이름**의 secret이 있으면 자식에 `AGENTVAULT_TICKET`을 export.
- `--env KEY=secret:NAME`: 시작 전에 `NAME`을 해석해 일반 env로 값 주입.
- `--session`: renew 가능한 부모를 만들고 다음을 export:
  - `AGENTVAULT_PARENT_TICKET`
  - `AGENTVAULT_PARENT_FILE`
- 자식 종료 후 session 부모는 폐기되고 백그라운드 renew도 중지됩니다.

> `init`이 `AGENTVAULT_TICKET`을 자동 export하지 않는 이유: 대부분의 MCP server는 그 변수를 모르고 `DATABASE_URL` 같은 일반 env만 읽습니다. `init`은 호환되는 "런타임 값 주입" 경로를 씁니다. 티켓 인식 custom server는 `run --name <같은 이름 secret>` 또는 `--session`을 사용하세요.

### 9단계: 업그레이드 후 launcher 새로고침

debug 빌드에서 release로 바꾸거나 설치 경로를 옮겼다면 env 재마이그레이션은 필요 없습니다.

```bash
agentvault init --agent codex --refresh --dry-run
agentvault init --agent codex --refresh
```

<p align="center">
  <img src="docs/assets/terminal/refresh-launcher.svg" alt="Refresh launcher" width="100%">
</p>

변경하는 것은 두 가지뿐입니다.

- 이미 래핑된 server의 launcher 경로;
- `--home` 값.

env, `run` 이후의 원래 명령, `--no-autostart` 같은 접미사는 **건드리지 않습니다**. JSON과 Codex TOML을 지원하며 각 파일은 먼저 백업됩니다.

### 10단계: 퇴사, 분실, 허니팟

**전역 kill switch:**

```bash
agentvault revoke --all
```

**하나의 secret 신규 발급 차단:**

```bash
agentvault revoke --secret postgres/DATABASE_URL
```

**티켓 하나 폐기(jti 또는 전체 token):**

```bash
agentvault revoke --ticket 5de9965d-...
agentvault revoke --ticket av1...
```

**로테이션:**

```bash
agentvault rotate --secret postgres/DATABASE_URL
```

새 값을 생성하고 secret epoch를 올려 이전 티켓을 resolve 시 실패하게 하며, 해당 secret의 폐기 상태를 해제합니다.

**허니팟:**

```bash
agentvault honeypot add prod/canary --json
# 반환된 token을 에이전트가 절대 읽으면 안 되는 .env / 설정에 둡니다
agentvault honeypot check <token>
```

<p align="center">
  <img src="docs/assets/terminal/honeypot-trip.svg" alt="Honeypot trip" width="100%">
</p>

실제 출력:

```text
hit=true honeypot=prod/canary revoked_all=true
```

히트하면 전역 폐기가 발생합니다. 이후 resolve는 다음을 반환합니다.

```text
error: denied: ticket was invalidated by a global revocation
```

**감사:**

```bash
agentvault audit --tail 20
```

```text
[1795261677681] ticket.resolve     actor=cli  secret=demo/token  ticket=a705ff08-...  ok
[1795261677724] ticket.resolve     actor=cli  secret=demo/token  ticket=a705ff08-...  revoked
[1795261686286] honeypot.triggered actor=cli  secret=canary      ticket=-             blocked
```

감사는추가형 JSONL입니다. 로테이션 시 이전 파일을 보존합니다.

### 11단계: 실제 테스트 실행

```bash
cargo test --workspace
cargo build --release
```

로컬 결과(Windows 11, 2026-11-22):

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

`1 ignored`는 Windows ConPTY 마스크 입력 통합 테스트입니다. 실제 대화형 터미널이 필요하며 통과를 꾸미지 않습니다.

<p align="center">
  <img src="docs/assets/diagrams/test-evidence.svg" alt="Test evidence" width="100%">
</p>

커버리지는 해피 패스가 아닙니다.

- JSON / Codex / Qoder 설정 재작성과 멱등성;
- 알 수 없는 필드 보존, 여러 줄 TOML args;
- 일회용, 만료, 재생 거부;
- task hard-TTL 상한, renew가 단축하지 않음;
- revoke token / jti / all, rotate 후 이전 티켓 무효화;
- 허니팟 전역 폐기;
- autostart 토글, 단일 인스턴스, broken pipe;
- 실제 Codex 설정 복사본에 대한 refresh;
- 사용자 범위 `env install`, 멱등성, 제거와 복원.

> Cursor/Qoder GUI 내부 도구 호출 자동화, macOS/Linux 실기 검증, sandbox/egress는 없습니다. 알려진 한계에 명시했으며 완료했다고 주장하지 않습니다.

---

## 설정 탐지와 env 마이그레이션

### 경로 탐지: 레지스트리이지 휴리스틱이 아님

- 각 클라이언트 후보는 **정적 레지스트리**이며 프로젝트 경로가 사용자 전역보다 먼저입니다.
- `init --list`는 모든 후보, 존재 여부, server 수, 래핑 수, `auto` 선택을 표시합니다.
- `--agent`로 클라이언트 강제, `--path`로 파일 지정.
- 재귀 스캔, 브라우저 기록, 추측은 없습니다.

### env 마이그레이션: 휴리스틱이지만 가시적·재정의 가능

판단 순서:

1. **참조 가드**: `${...}`, `$VAR`, `%VAR%`, `{{...}}`, 그리고 `${HOME}/.config` 같은 내장 형태 → 절대 마이그레이션하지 않음.
2. **key 마커**: `TOKEN`, `SECRET`, `PASSWORD`, `API_KEY`, `DATABASE_URL`, `DSN`, `WEBHOOK`, `COOKIE` 등.
3. **값 형태**: `sk-`, `ghp_`, JWT, PEM, `postgres://user:pass@...`, 긴 랜덤 문자열 등.

휴리스틱은 틀릴 수 있습니다. 그래서 세 가지 안전장치가 있습니다.

```bash
agentvault init --dry-run                 # 확인
agentvault init --migrate-env KEY         # 강제 마이그레이션
agentvault init --keep-env KEY            # 강제 유지
```

`init --dry-run`은 `migrate env` / `keep env` / `unchanged env` / `skip`을 출력합니다. 실제 쓰기는 항상 먼저 백업합니다.

### 백업과 실패 복구

- 쓰기 전: 암호화 백업 `<config>.bak.<ms>.enc`(`AVBAK1` + ChaCha20-Poly1305, 키는 OS 키스토어).
- 쓰기 후: 재파싱해 래핑 결과를 검증하고 실패 시 백업에서 복원.
- `backup list`는 마이그레이션/정리를 위해 기존 평문 `*.bak.*`도 표시합니다.
- `init --refresh --cleanup-backups`로 확인 후 백업을 정리할 수 있습니다.

---

## 명령어 목록

| 명령 | 목적 | 주요 옵션 |
|---|---|---|
| `init` | MCP 설정 탐지·래핑, secret env 마이그레이션 | `--dry-run` `--list` `--refresh` `--agent` `--path` `--migrate-env` `--keep-env` `--no-env` `--rebind` |
| `doctor` | home, broker, keystore, agent 설정 진단 | `--home` |
| `env show` | home 출처와 영구 값 표시 | `--home` |
| `env install` | `AGENTVAULT_HOME` + PATH 영구화 | `--scope user\|machine` `--no-path` `--dry-run` |
| `env uninstall` | 영구 값 제거 | `--scope user\|machine` `--dry-run` |
| `secret add` | 장기 시크릿 저장 | `--value` `--stdin` `--kind` `--honeypot` |
| `secret list` | 시크릿 목록(값 미표시) | `--home` |
| `secret remove` | 시크릿 삭제 | `<NAME>` |
| `secret unrevoke` | 로테이션 없이 폐기된 이름 재발급 허용 | `<NAME>` |
| `issue` | 단기 티켓 발급 | `--secret` `--ttl` `--scope` `--max-uses` `--task` `--hard-ttl` `--json` |
| `resolve` | 티켓 교환(기본 1회) | `--ticket` `--json` |
| `child` | 부모에서 리프 티켓 발급 | `--parent` / `--parent-file` `--ttl` `--sub` `--max-uses` |
| `renew` | 부모 만료 연장, hard-ttl 상한 | `--ticket` `--ttl` |
| `run` | 프로세스 래핑, 티켓 또는 해석 값 주입 | `--name` `--env KEY=secret:NAME` `--session` `--ttl` `--hard-ttl` |
| `revoke` | 티켓 / secret / 전체 폐기 | `--ticket` `--secret` `--all` |
| `rotate` | 값 로테이션 및 이전 티켓 무효화 | `--secret` |
| `status` | secret, 바인딩된 agent, live ticket 표시 | `--json` |
| `audit` | JSONL 감사 표시 | `--tail N` `--json` `--event` `--since` `--export` `--verify` `--verify-signature` `--pubkey` |
| `honeypot add/list/check` | 허니팟 canary 관리 | `--json` |
| `backup create/restore/list` | 설정 암호화 백업, 복원, 목록 | `--path` `--backup` |
| `web` | 로컬 Web 콘솔 | `--port` `--dist`; 원격 바인딩은 `AGENTVAULT_WEB_BIND` + `AGENTVAULT_WEB_ALLOW_REMOTE=1` |
| `daemon` | broker 포그라운드 실행 | `--detach` |
| `stop` | 백그라운드 broker 중지 | `--home` |

broker가 필요한 명령은 기본적으로 detached broker를 자동 시작합니다. `--no-autostart` 또는 `AGENTVAULT_NO_AUTOSTART=1`로 끌 수 있습니다.

---

## 첫 실행과 AGENTVAULT_HOME

기본 상태 디렉터리:

| 플랫폼 | 경로 |
|---|---|
| Windows | `%LOCALAPPDATA%\agentvault` |
| macOS | `~/Library/Application Support/agentvault` |
| Linux | `$XDG_DATA_HOME/agentvault` 또는 `~/.local/share/agentvault` |

우선순위: `--home` > `AGENTVAULT_HOME` > 플랫폼 기본값.

첫 실제 `init`(`--home` 재정의가 아닌 플랫폼 기본값 사용)은:

- `AGENTVAULT_HOME`을 **user scope**에 영구화;
- 바이너리 디렉터리를 **user PATH**에 추가;
- machine-wide 변수를 조용히 쓰지 않음; 필요 시 `env install --scope machine` 명시(보통 권한 상승 필요).

관리:

```bash
agentvault env show
agentvault env install --dry-run
agentvault env install
agentvault env install --scope machine
agentvault env uninstall
agentvault init --no-env   # 첫 init이 환경 변수를 건드리지 않게
```

주의:

- 이미 열린 터미널에는 반영되지 않습니다. 새 터미널 / 재로그인 후 적용됩니다.
- `--home` 또는 `AGENTVAULT_HOME`을 쓰면 첫 init이 그 임시 경로를 전역 기본값으로 영구화하지 않습니다.
- 래핑된 server는 명시적 `--home`을 가져 agent의 PATH/env가 달라도 같은 vault를 읽습니다.

---

## 실제 테스트 증거

"단위 테스트 몇 개 쓰고 끝"이 아닙니다. 실제 Windows 11 머신에서:

- **Rust 테스트 54개 통과, ConPTY 1개는 명시적 ignored**: `cargo test --workspace`.
- **브라우저 수준 Playwright 테스트 2개**: token 일회성 부트스트랩, 로그인·시크릿 추가·티켓 발급·Agent 경로 선택·감사 필터.
- **150개 격리 CLI 검사**: 각각 독립 `--home`으로 init, ticket, revoke, rotate, env, doctor, autostart, broken pipe를 커버.
- **실제 Codex 설정 복사본**: 실제 5003바이트 `config.toml`을 바이트 단위로 백업하고 SHA-256 기록, init/refresh 후 `codex mcp get`으로 launcher와 home 검증.
- **실제 Cursor / Qoder 탐색**: 경로 레지스트리, 빈 설정, 프로젝트 우선순위 확인.
- **실제 환경 변수 쓰기와 복원**: user scope `env install`, 멱등성, `env uninstall` 복원.
- **실제 공격 의미론**: 재생 거부, 만료 거부, 전역 폐기, 로테이션 무효화, 허니팟 트리거.
- **새 회귀**: 전체 TOML 왕복, 암호화 백업, Ed25519 감사 서명, Web 역할과 viewer 읽기 전용.

<p align="center">
  <img src="docs/assets/terminal/replay-denied.svg" alt="Replay denied" width="100%">
</p>

<p align="center">
  <img src="docs/assets/terminal/honeypot-trip.svg" alt="Honeypot trip" width="100%">
</p>

---

## 보안 모델

<p align="center">
  <img src="docs/assets/diagrams/security-layers.svg" alt="Security model" width="100%">
</p>

### L0: 시크릿 보관

- Windows: DPAPI(PowerShell 경유, 추가 native 빌드 의존성 없음).
- macOS: Keychain.
- Linux: Secret Service. headless는 0600 암호화 파일로 폴백.
- `status`가 현재 backend를 표시합니다.

### 장치 및 하드웨어 바인딩

- 장치 지문: hostname + user.
- `init --rebind`로 하드웨어 지문(WMI UUID / IOPlatformUUID / DMI product UUID) 기록.
- 불일치 시 broker가 시작을 거부.
- TPM sealing이 아닙니다. TPM은 향후 enterprise 방향입니다.

### 티켓

- HMAC-SHA256. 키는 master key + 전역 폐기 epoch에서 파생.
- `revoke --all`은 전역 epoch를 올려 모든 이전 티켓을 한 번에 폐기.
- secret 로테이션은 secret epoch를 올려 이전 티켓을 resolve에서 실패시킴.
- 기본 `max_uses=1`; 소진 시 자동 폐기.
- task 티켓은 `hard_exp`를 가지며 `renew`가 넘을 수 없음.
- 자식 만료는 부모 `hard_exp`가 상한.
- `revoke --ticket`은 jti 또는 전체 token을 받음.

### 로컬 broker

- `127.0.0.1`에만 바인딩.
- 단일 인스턴스 잠금.
- `admin.token`은 owner-only ACL(Windows `icacls`, Unix 0600).
- 복호화된 secret은 메모리에 캐시; 매번 PowerShell을 띄우지 않음.
- broker 종료 후 다음 필요 명령이 자동 시작.

### 감사와 허니팟

- 추가형 JSONL: 시간, actor, 이벤트, secret, ticket, 결과.
- `audit --tail N`으로 조회.
- 각 줄에 `prev_hash`/`hash` 해시 체인. `audit --verify`가 첫 단절을 보고.
- 각 줄에 Ed25519 서명. `audit --verify-signature` 검증, `audit --pubkey` 공개키 출력.
- 로테이션 시 이전 감사 파일 보존.
- 허니팟 token은 고유하며 히트 시 전역 폐기.
- 허니팟 check는 실제 secret을 노출하지 않습니다.

### Web 콘솔

- 기본은 `127.0.0.1`만. 비 loopback은 명시 설정이고 최소 1개 계정이 필요합니다.
- `web.token`은 로컬 세션, 비밀번호는 Argon2id, 세션 token은 SHA-256으로 저장.
- owner/admin은 쓰기 가능, viewer는 서버에서 읽기 전용으로 강제.
- 테넌트는 필터용 라벨이며 강한 격리 경계가 아닙니다.
- 페이지는 장기 secret 값을 반환하지 않고 SSE는 상태와 이벤트만 전송합니다.

### 암호화 백업

- `AVBAK1` + ChaCha20-Poly1305.
- 키는 OS 키스토어의 `backup/master`.
- 복원 전 JSON/TOML 재파싱, 실패 시 대상 파일을 깨뜨리지 않습니다.
- `init --cleanup-backups`는 암호화 백업과 기존 평문 `.bak.*`를 정리합니다.

---

## 알려진 한계

v0.1의 실제 한계입니다. 각주에 숨기지 않습니다.

1. **샌드박스가 아님**: 같은 OS 사용자의 악성 프로세스는 사용자 파일, `admin.token`, 백업을 읽을 수 있습니다.
2. **broker는 로컬 TCP**: 원격 서비스가 아닙니다. `admin.token`을 읽는 로컬 프로세스는 관리할 수 있습니다.
3. **장치 바인딩은 TPM이 아님**: 지문 검사이며 하드웨어 봉인이 아닙니다.
4. **미마이그레이션 평문은 범위 밖**: v0.1은 아는 설정만 처리합니다. 스크립트, `.env`, 로그의 평문은 범위 밖입니다.
5. **env 마이그레이션은 휴리스틱**: key 마커 + 값 형태로 오판할 수 있습니다. dry-run + `--migrate-env` / `--keep-env`를 사용하세요.
6. **Codex TOML은 `toml_edit` 왕복**: 따옴표, 이스케이프, 여러 줄 배열, inline table, dotted key, 주석, CRLF, BOM을 커버. 문자열이 아닌 args/env는 `unsupported`로 건너뜁니다.
7. **JSON은 strict 필수**: JSONC 주석/끝 쉼표는 명확한 오류로 거부. UTF-8 BOM은 허용.
8. **`~/.claude.json`은 최상위만**: `projects.*.mcpServers`는 자동 재작성하지 않습니다. 프로젝트 `.mcp.json`에 `--path`를 사용하세요.
9. **`init`은 기본적으로 값 주입, 티켓 아님**: stock MCP server 호환. 티켓 인식은 `run --name <같은 이름 secret>` 또는 `--session`.
10. **GUI 호출 자동화 없음**: Cursor/Qoder GUI 내부 도구 호출은 자동화하지 않았습니다. CLI, 설정, Web 클릭 경로는 검증했습니다.
11. **macOS/Linux 실기 검증 미완료**: 코드와 단위 테스트는 크로스 플랫폼이지만 실기 실행은 Windows 11입니다.
12. **Web 콘솔은 로컬 우선**: 계정, 역할, 테넌트 라벨이 있지만 강화된 원격 멀티테넌트 서비스가 아닙니다. 원격 노출에는 TLS, 리버스 프록시, 네트워크 ACL, 백업 정책이 필요합니다.
13. **ConPTY 마스크 입력 테스트는 ignored**: 실제 대화형 터미널이 필요하며 통과를 꾸미지 않습니다.

---

## FAQ

### Q1: 시스템 환경 변수에 token을 두는 것과 무엇이 다른가요?

시스템 환경 변수는 여전히 장기 평문이며 같은 사용자의 모든 프로세스가 읽을 수 있고 TTL, 횟수 제한, 폐기, 감사가 없습니다. AgentVault는 장기 값을 OS 키스토어에 두고 에이전트에는 만료·폐기 가능한 단기 티켓을 줍니다.

### Q2: `init`이 설정을 망가뜨릴 수 있나요?

dry-run 계획, 백업, 계획된 server만 변경, 쓰기 후 재파싱, 실패 시 복원을 수행합니다. 알 수 없는 필드도 보존합니다. 그래도 프로덕션 설정은 먼저 복사하거나 `--path`로 복사본에 시험하세요.

### Q3: `${HOME}/.config`가 왜 마이그레이션되지 않나요?

의도적입니다. 참조이지 시크릿이 아닙니다. 마이그레이션하면 확장이 리터럴이 되어 MCP 클라이언트 동작을 깨뜨립니다. `keep env`에 표시됩니다.

### Q4: `init` 후 왜 티켓이 아니라 값이 전달되나요?

대부분의 MCP server는 `DATABASE_URL` / `API_KEY` 같은 일반 env만 읽습니다. `init`은 호환 경로로 시작 시 티켓을 값으로 교환해 env에 주입합니다. 티켓 인식 custom server는 `run --name <같은 이름 secret>` 또는 `--session`을 사용하세요.

### Q5: `revoke --all` 후에도 사용할 수 있나요?

네. 전역 폐기는 epoch를 올리고, 이후 발급된 티켓은 새 epoch로 정상 동작합니다. 이전 티켓은 모두 무효입니다.

### Q6: revoke와 rotate의 차이는?

- `revoke --secret`: 해당 secret의 신규 발급 중지. 기존 티켓도 실패. 값은 남음.
- `rotate --secret`: 새 값 생성, epoch 상승, 이전 티켓 무효화, 해당 secret의 폐기 상태 해제.
- `revoke --all`: 모든 secret의 모든 이전 티켓을 한 번에 무효화.

### Q7: broker가 매 도구 호출을 느리게 하나요?

broker는 복호화된 secret을 메모리에 캐시하고 DPAPI는 시작 시 한 번만 호출합니다. 일반 issue/resolve는 로컬 TCP + HMAC 검증으로 가볍습니다.

### Q8: 내 MCP server에 연결할 수 있나요?

네, 두 가지 방법:

```bash
# 1. 일반 env 주입 (stock server 호환)
agentvault run --name my-server --env API_KEY=secret:my/API_KEY -- node server.js

# 2. 티켓 인식
agentvault run --name my/API_KEY -- node server.js
# 자식은 AGENTVAULT_TICKET을 읽고 agentvault resolve 호출
```

### Q9: Web 콘솔과 CLI의 관계는?

CLI는 가장 작은 신뢰 표면이자 자동화 진입점입니다. Web 콘솔은 같은 바이너리 안의 로컬 시각화 계층으로, 같은 `AGENTVAULT_HOME`과 broker를 읽습니다. 기본은 `127.0.0.1`만. 처음에는 `web.token`으로 로그인하고 설정에서 owner/admin/viewer 계정을 만듭니다. 클라우드 SaaS가 아니며 기본적으로 원격 노출도 하지 않습니다.

### Q10: 팀이 하나의 vault를 공유할 수 있나요?

v0.1은 한 머신에서 owner/admin/viewer 계정과 테넌트 라벨을 제공하며, 소규모 팀이 워크스테이션을 공유하는 용도에 맞습니다. 같은 home을 네트워크 공유에 두고 동시에 사용하는 것은 **지원하지 않습니다**. 머신 간 강한 격리도 보장하지 않습니다. 중앙 정책, 원격 감사, 진짜 멀티테넌시는 향후 작업입니다.

---

## 저장소 구조

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

주요 진입점:

- `crates/cli/src/main.rs`: 명령 파싱, init/issue/run/doctor/web 등.
- `crates/adapters/src/lib.rs`: 경로 레지스트리, JSON plan/apply/refresh, env 휴리스틱.
- `crates/adapters/src/codex.rs`: Codex TOML 전체 왕복과 실패 시 거부.
- `crates/backup/src/lib.rs`: 암호화 백업과 복원.
- `crates/audit/src/lib.rs`: 해시 체인과 Ed25519 서명.
- `crates/broker/src/lib.rs`: 로컬 broker와 티켓 수명주기.
- `crates/core/src/token.rs`: HMAC 티켓 서명/검증.
- `crates/keystore/src/lib.rs`: DPAPI / Keychain / Secret Service / file backend.
- `crates/web/src/lib.rs`, `webapp/src/App.tsx`: 콘솔 API와 UI.

---

## 로드맵

- [x] L0: OS 키스토어, 장치 바인딩, 하드웨어 지문, 서명된 티켓
- [x] L1: 로컬 broker, 단기 티켓, task/child/renew, rotate/revoke
- [x] L4: JSONL 감사, Ed25519 서명, 허니팟 canary, 전역 폐기
- [x] `init --refresh`: 업그레이드 후 launcher/home 갱신
- [x] 내장 env 참조 가드(`${HOME}/...`, `%VAR%\...`)
- [x] Web 콘솔: 단일 바이너리, 계정/역할/테넌트 라벨, SSE, 경로 선택
- [x] 암호화 백업(AVBAK1), 전체 TOML 왕복, 감사 서명 검증
- [ ] 데스크톱 셸: 로컬 프리뷰 완료, 아직 저장소에 커밋하지 않음
- [ ] L2: 프로세스 샌드박스 / microVM
- [ ] L3: 이그레스 allowlist / DLP
- [ ] 원격 Web 배포 강화 / 중앙 정책 / 강한 멀티테넌시
- [ ] macOS/Linux 실기 검증과 CI 커버리지
- [ ] TPM / Secure Enclave 바인딩
- [ ] 팀 정책과 중앙 감사

---

## 기여 및 보안

- 기여 가이드: [CONTRIBUTING.md](CONTRIBUTING.md)
- 보안 정책과 취약점 보고: [SECURITY.md](SECURITY.md)
- issue / PR에 실제 token, cookie, 프로덕션 설정을 붙여넣지 마세요. `sk-test-...` 합성 값을 사용하세요.
- 설정 재작성은 반드시 백업을 유지하고 `*.bak.*`를 commit하지 마세요.
- 중국어 README가 기준입니다. 동작 변경 시 EN/JA/KO도 업데이트하세요.

---

## 라이선스

Apache License 2.0. [LICENSE](LICENSE)를 참조하세요.

---

<p align="center">
  <sub>Built for agents that should borrow credentials, not own them.</sub>
</p>

<p align="center">
  <a href="https://star-history.com/#Seramys/agentvault&Date">
    <img src="https://api.star-history.com/svg?repos=Seramys/agentvault&type=Date" alt="Star history" width="720">
  </a>
</p>