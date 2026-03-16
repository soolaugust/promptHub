# PromptHub

A layered prompt management system inspired by Docker. Compose reusable prompt layers into production-ready AI prompts.

[中文文档](README.zh.md)

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)
[![CI](https://github.com/soolaugust/promptHub/actions/workflows/ci.yml/badge.svg)](https://github.com/soolaugust/promptHub/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/prompthub.svg)](https://crates.io/crates/prompthub)
[![crates.io downloads](https://img.shields.io/crates/d/prompthub.svg)](https://crates.io/crates/prompthub)
[![Demo](https://img.shields.io/badge/demo-online-brightgreen)](https://prompthub-demo.up.railway.app)

![PromptHub demo](docs/demo.gif)

```
╔═══════════════════════════════════════════════════╗
║  Promptfile                                       ║
║                                                   ║
║  FROM   base/code-reviewer:v1.0                  ║
║  LAYER  style/concise:v1.0                       ║
║  LAYER  guard/no-secrets:v1.0                    ║
║  VAR    language "中文"                           ║
║  TASK   "Review this pull request."              ║
╚══════════════════════╦════════════════════════════╝
                       ║
                    ph build
                       ║
                       ▼
╔═══════════════════════════════════════════════════╗
║  Merged Prompt                                    ║
║                                                   ║
║  [role]          You are a senior code reviewer…  ║
║  [constraints]   Be concise and direct…           ║
║  [constraints]   Never output secrets…            ║
║  [output-format] ## Critical Issues…              ║
║                                                   ║
║  ---                                              ║
║  用中文审查这个 Pull Request。                     ║
╚═══════════════════════════════════════════════════╝
```

## How it works

```
  layers/                          Promptfile
  ├── base/                        ─────────────────────────────
  │   └── code-reviewer/           FROM  base/code-reviewer:v1.0
  │       ├── layer.yaml    ──▶    LAYER style/concise:v1.0
  │       └── prompt.md            LAYER guard/no-secrets:v1.0
  ├── style/                       VAR   language "中文"
  │   └── concise/          ──▶   TASK  "审查代码"
  └── guard/
      └── no-secrets/       ──▶
                    │
                 ph build
                    │
                    ▼
  ╔══════════════════════════════════════════════╗
  ║  [role]          ◀ from  code-reviewer       ║
  ║  [constraints]   ◀ overridden by  concise    ║  ← same key: later wins
  ║  [constraints]   ◀ appended by    no-secrets ║  ← new key:  appended
  ║  [output-format] ◀ from  code-reviewer       ║
  ║  ─────────────────────────────────────────── ║
  ║  审查代码                                     ║
  ╚══════════════════════════════════════════════╝
```

Layers merge deterministically: **same section name → later layer overrides**, **new section name → appended**. Variables (`${language}`) are substituted at build time.

## Why PromptHub

**vs. hand-written prompts:**
- **Reuse without copy-paste** — one layer, many Promptfiles. Fix a bug once, all builds pick it up.
- **Versioned and auditable** — `ph diff` shows exactly what changed between builds; `ph build -o json` emits a reproducible digest.
- **Team-shareable** — push layers to a private registry; teammates pull the exact version you tested.

**Prompt footprint** (4 skills sharing 3 common layers, measured):

| | Copy-paste (no PromptHub) | With PromptHub |
|---|---|---|
| Repo storage | ~12 KB (4 full prompts, each a separate file) | ~1.7 KB Promptfiles + shared layers stored once |
| Tokens sent to LLM | ~600–850 tokens / skill | Identical (`ph build` expands to the same content) |
| Fix a bug in a shared layer | Edit N files manually | **Edit 1 file — all skills pick it up** |
| Version pinning | Comments or git blame | `FROM base/office-doc:v1.0` in the Promptfile |
| Team sync | Copy files manually | `ph pull` — exact tested version, every time |

## Try It Online

[Open Registry UI →](https://prompthub-demo.up.railway.app)

Or pull a layer directly (no account required):

```bash
ph pull base/code-reviewer:v1.0 --source https://prompthub-demo.up.railway.app
```

> **Note:** The demo runs on Render's free tier and may take ~30 seconds to wake up after inactivity.

## Installation

```bash
cargo install --path prompthub/
```

This installs two binaries:
- `ph` — CLI tool
- `ph-mcp` — MCP server for AI assistants (Claude, Cursor, etc.)

To also build the private registry server:

```bash
cargo install --path registry/
```

## Quick Start

```bash
# 1. Pull a layer from the official registry
ph pull base/code-reviewer:v1.0

# 2. Create a Promptfile
ph init

# 3. Build and use the prompt
ph build
```

```
$ ph build
[role]
You are a senior code reviewer with 10+ years of experience.

[constraints]
- Focus on logic errors and security vulnerabilities
- Provide specific fix suggestions
...

$ ph build -o json
{
  "prompt": "[role]\nYou are a senior code reviewer...",
  "params": { "model": "claude-sonnet-4-6", "temperature": "0.3" },
  "layers": ["base/code-reviewer:v1.0", "style/concise:v1.0"],
  "digest": "sha256:a1b2c3..."
}
```

Pipe `ph build -o json` directly into your CI pipeline or API call — no copy-paste, no drift.

## Promptfile Syntax

```
FROM base/code-reviewer:v1.0      # Base layer (required, must be first)
LAYER style/concise:latest         # Additional layers (merged in order)
LAYER guard/no-secrets:v1

VAR language "中文"                 # Variable with default (override with --var)
PARAM model "claude-sonnet-4-6"    # Build-time parameter (included in JSON output)
PARAM temperature "0.3"

INCLUDE ./context.md               # Inline a local file

TASK "用${language}审查这段代码"    # Task appended at the end of the prompt
```

| Directive | Syntax | Description |
|-----------|--------|-------------|
| `FROM` | `FROM <layer>:<version>` | Base layer. Required, must be first, only one allowed. |
| `LAYER` | `LAYER <layer>:<version>` | Additional layer. Multiple allowed, merged in order. |
| `PARAM` | `PARAM <key> "<value>"` | Build parameter (model, temperature, etc.). |
| `VAR` | `VAR <name> "<default>"` | Variable. Override at build time with `--var name=value`. |
| `TASK` | `TASK "<text>"` | Task description appended to the final prompt. |
| `INCLUDE` | `INCLUDE <file>` | Inline a local file's content. |
| `#` | `# comment` | Comment line. |

### Version Syntax

| Spec | Matches |
|------|---------|
| `layer:v1.0` | Exact version |
| `layer:v1` | Latest v1.x |
| `layer:latest` or `layer` | Latest available |

## Layer Specification

A layer is a directory with two files:

```
layers/base/code-reviewer/
  layer.yaml       # Metadata
  prompt.md        # Content with [section] markers
```

### layer.yaml

```yaml
name: code-reviewer
namespace: base
version: v1.0
description: "Professional code reviewer"
author: prompthub
tags: [code, review]
sections: [role, constraints, output-format]   # Sections defined in prompt.md
conflicts: [base/translator]                    # Incompatible layers
requires: []                                    # Required layers
models: [claude-*, gpt-4*]                      # Compatible models (glob)
```

### prompt.md

Sections are delimited by `[section-name]` markers:

```markdown
[role]
You are a senior code reviewer with 10+ years of experience.

[constraints]
- Focus on logic errors and security vulnerabilities
- Provide specific fix suggestions

[output-format]
## Issues
- **[CRITICAL]** `file:line` — description
## Summary
Overall assessment.
```

### Merge Rules

- **Same section name** → later layer overrides earlier (with a warning)
- **New section name** → appended to the merged prompt

## Official Layers

| Layer | Description |
|-------|-------------|
| `base/code-reviewer` | Professional code review expert |
| `base/translator` | Multi-language translator with cultural adaptation |
| `base/writer` | Clear and engaging professional writer |
| `base/analyst` | Rigorous data analyst |
| `style/concise` | Short, direct responses |
| `style/verbose` | Thorough, step-by-step explanations |
| `style/academic` | Formal academic writing style |
| `lang/chinese-markdown` | Simplified Chinese + Markdown output |
| `lang/english-academic` | Formal English academic format |
| `lang/structured-output` | Machine-parseable structured output |
| `guard/no-secrets` | Prevent exposure of sensitive information |
| `guard/safe-output` | General safety constraints |
| `guard/fact-check` | Enforce factual accuracy and uncertainty acknowledgment |

## CLI Reference

```bash
# Layer management
ph layer new base/my-role          # Create a new layer template
ph layer list                      # List all locally available layers
ph layer inspect base/code-reviewer  # Show metadata and content
ph layer validate base/code-reviewer # Validate layer format

# Fetching layers
ph pull base/code-reviewer:v1.0    # Pull from registry (cached in ~/.prompthub/layers/)
ph push base/my-expert:v1.0        # Push to registry

# Build & compare
ph build                           # Build to stdout
ph build -o json                   # Structured output (prompt + params + digest)
ph build --var language=English    # Override a variable
ph diff Promptfile Promptfile.prod # Compare two Promptfiles

# Other
ph search translation              # Search layers by keyword
ph history base/code-reviewer      # Show locally cached versions
ph login --token <tok> <url>       # Authenticate to a registry
```

Configure additional sources in `~/.prompthub/config.yaml`:

```yaml
sources:
  - name: official
    url: https://raw.githubusercontent.com/prompthub/layers/main
    default: true
  - name: my-team
    url: https://registry.mycompany.internal
    auth:
      token: phrt_xxxxxxxxxxxx
```

## Private Registry

For teams that need to self-host layers internally, PromptHub includes `ph-registry` — a standalone HTTP server with S3/filesystem storage, SQLite metadata, and token-based auth.

```
  Developer / CI Pipeline / AI Agent
                   │
                   │  HTTPS
                   ▼
  ╔══════════════════════════════════════════════════════╗
  ║  ph-registry  (Axum · Rust)                          ║
  ║                                                      ║
  ║  GET  /layers/{ns}/{name}/{ver}/layer.yaml           ║
  ║  GET  /layers/{ns}/{name}/{ver}/prompt.md            ║
  ║  PUT  /layers/{ns}/{name}/{ver}          (push)      ║
  ║  GET  /layers?q=keyword                  (search)    ║
  ║  POST /v1/auth/login                                 ║
  ║  POST /v1/auth/token                     (admin)     ║
  ║                                                      ║
  ║  ┌─────────────────┐     ┌──────────────────────┐   ║
  ║  │   SQLite DB     │     │  S3 · MinIO · FS     │   ║
  ║  │  ▸ users        │     │  layers/             │   ║
  ║  │  ▸ tokens       │     │  └── {ns}/{name}/    │   ║
  ║  │  ▸ layer_meta   │     │      └── {ver}/      │   ║
  ║  └─────────────────┘     └──────────────────────┘   ║
  ╚══════════════════════════════════════════════════════╝
```

### Start the registry

**Filesystem storage (single machine, no dependencies):**

```yaml
# registry.yaml
server:
  port: 8080
storage:
  type: filesystem
  path: /var/lib/prompthub/layers
database:
  path: /data/registry.db
auth:
  pull_requires_auth: false
  admin_token: "phrt_bootstrap_changeme"
log:
  level: info
```

```bash
ph-registry registry.yaml
# ph-registry listening on 0.0.0.0:8080
```

**Docker Compose (production, with MinIO):**

```bash
docker compose up
# Starts ph-registry on :8080 + MinIO on :9000
```

See [`registry/docker-compose.yaml`](registry/docker-compose.yaml) for the full configuration.

### Authenticate

```bash
# Non-interactive (CI / AI agents) — use an admin-issued token directly
ph login --token phrt_xxxxxxxxxxxx https://registry.mycompany.internal
# ✓ Logged in to registry.mycompany.internal

# Interactive — prompts for username + password
ph login https://registry.mycompany.internal

# Remove token
ph logout https://registry.mycompany.internal
```

### Push a layer

```bash
ph push base/my-expert:v1.0
# ✓ Pushed base/my-expert:v1.0 to my-company

# Push to a specific named source
ph push --source my-company base/my-expert:v1.0

# Version immutability — pushing the same version twice is rejected
```

`ph push` validates the layer locally before sending it, so bad layers are rejected before any network round-trip.

### Issue tokens (admin)

```bash
curl -X POST https://registry.mycompany.internal/v1/auth/token \
  -H "Authorization: Bearer phrt_bootstrap_changeme" \
  -H "Content-Type: application/json" \
  -d '{"name": "ci-pipeline", "expires_in_days": 365}'
# {"token": "phrt_abc123...", "name": "ci-pipeline", "expires_at": "2027-03-16T..."}
```

### Registry workflow demo

![PromptHub registry demo](docs/demo-registry.gif)

### Web UI

`ph-registry` ships with a built-in web interface. Once the registry is running, open `http://localhost:8080` in your browser:

![PromptHub Registry Dashboard](docs/demo-ui.png)

| Layers browser | Layer detail |
|---|---|
| ![Layers](docs/demo-ui-layers.png) | ![Detail](docs/demo-ui-detail.png) |

Browse layers, inspect prompt content, and view version history — no extra tooling required.

### Full workflow example

```
# 1. Start the registry (one-time, ops team)
ph-registry registry.yaml

# 2. Authenticate (one-time per machine)
ph login --token phrt_abc123 https://registry.mycompany.internal

# 3. Author a layer locally
ph layer new base/sql-expert
# Edit layers/base/sql-expert/layer.yaml and prompt.md ...

# 4. Push to private registry
ph push base/sql-expert:v1.0
# ✓ Pushed base/sql-expert:v1.0 to my-company

# 5. Anyone on the team can now pull it
ph pull base/sql-expert:v1.0
# ✓ Pulled base/sql-expert:v1.0 to ~/.prompthub/layers/base/sql-expert/v1.0

# 6. Use in a Promptfile
cat Promptfile
# FROM base/sql-expert:v1.0
# LAYER style/concise:v1.0
# TASK "Optimize the following query for PostgreSQL."

ph build
```

## MCP Server

`ph-mcp` is an MCP (Model Context Protocol) server that lets AI assistants like Claude and Cursor use PromptHub directly — no copy-paste required.

```
  Claude Desktop / Cursor / Claude Code
           │
           │  MCP (stdio)
           ▼
  ╔═════════════════════════════════════════════╗
  ║  ph-mcp                                     ║
  ║                                             ║
  ║  build_prompt  ──▶  parse → resolve         ║
  ║                            ↓                ║
  ║                         merge → render      ║
  ║  list_layers   ──▶  scan local + global     ║
  ║  search_layers ──▶  filter name/desc/tags   ║
  ║  inspect_layer ──▶  metadata + content      ║
  ╚═════════════════════════════════════════════╝
           │
           ▼
  ~/.prompthub/layers/  +  ./layers/  (project-local)
```

**Claude Desktop** — add to `~/Library/Application Support/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "prompthub": {
      "command": "ph-mcp"
    }
  }
}
```

**Cursor** — add to `.cursor/mcp.json` in your project:

```json
{
  "mcpServers": {
    "prompthub": {
      "command": "ph-mcp"
    }
  }
}
```

| Tool | Description |
|------|-------------|
| `build_prompt` | Build a prompt from a Promptfile path or inline content, with optional `--var` overrides |
| `list_layers` | List all locally available layers (project + global cache) |
| `search_layers` | Search layers by keyword across name, description, and tags |
| `inspect_layer` | Show full metadata and prompt content for a specific layer |

## Works Well With

| Tool | How |
|------|-----|
| [Claude Code](https://github.com/anthropics/claude-code) | Use `ph-mcp` as an MCP server; define skill system prompts as Promptfiles |
| [Cursor](https://cursor.com) | Use `ph-mcp` as an MCP server |
| Any CI/CD pipeline | `ph build -o json` outputs structured prompt + model params |
| Private team registries | `ph push` / `ph pull` for versioned, shared layers |

## Real-World Validation

We rebuilt four skills from [anthropics/skills](https://github.com/anthropics/skills) using PromptHub. Three genuine shared content blocks emerged:

| Shared Layer | Duplicated Across | What It Contains |
|---|---|---|
| `office-toolkit` | `docx`, `pptx`, `xlsx` | LibreOffice scripts, unpack/repack workflow |
| `office-quality` | `docx`, `xlsx` | Zero-error rule, Arial font, source documentation format |
| `anti-slop` | `frontend-design`, `pptx` | Anti-generic-AI-aesthetics design constraints |

The same content that lived in 3 separate skill files now lives in one layer. A single edit to `office-toolkit/prompt.md` propagates to all three Office skills on the next build.

Not all skills benefit. The `mcp-builder` skill's four-phase workflow (Research → Implement → Test → Evaluate) is a tightly coupled whole — splitting it into layers would break the logical flow. **PromptHub adds value where genuine shared content exists, not as a universal wrapper.**

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=soolaugust/promptHub&type=Date)](https://star-history.com/#soolaugust/promptHub&Date)

## License

MIT
