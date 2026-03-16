# PromptHub

A layered prompt management system inspired by Docker. Compose reusable prompt layers into production-ready AI prompts.

[中文文档](README.zh.md)

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)

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
# Pull a layer from the official registry
ph pull base/code-reviewer:v1.0
```

```
$ ph init
✓ Created Promptfile

$ ph build
[role]
You are a senior code reviewer with 10+ years of experience.

[constraints]
- Focus on logic errors and security vulnerabilities
- Provide specific fix suggestions
...

$ ph build --var language=English -o json
{
  "prompt": "[role]\nYou are a senior code reviewer...",
  "params": { "model": "claude-sonnet-4-6", "temperature": "0.3" },
  "layers": ["base/code-reviewer:v1.0", "style/concise:v1.0"],
  "digest": "sha256:a1b2c3..."
}
```

```bash
# List locally available layers
ph layer list

# Validate a layer's format
ph layer validate base/code-reviewer

# Compare build output of two Promptfiles
ph diff Promptfile Promptfile.prod
```

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

## Layer Management

```bash
# Create a new layer template
ph layer new base/my-role

# List all locally available layers
ph layer list
# Output:
# LAYER                     PATH
# ─────────────────────────────────────────────────────────────
# base/code-reviewer        ~/.prompthub/layers/base/code-reviewer/v1.0
# style/concise             ~/.prompthub/layers/style/concise/v1.0
# guard/no-secrets (local)  layers/guard/no-secrets/v1.0
# 3 layer(s) found

# Inspect a layer's metadata and content
ph layer inspect base/code-reviewer

# Validate a layer's format
ph layer validate base/code-reviewer
# ✓ Layer 'base/code-reviewer' is valid
```

## Fetching Layers

```bash
# Pull from the official registry
ph pull base/code-reviewer:v1.0
# Pulling base/code-reviewer:v1.0 from official...
# ✓ Pulled base/code-reviewer:v1.0 to ~/.prompthub/layers/base/code-reviewer/v1.0

ph pull style/concise
```

By default, layers are fetched from the official registry and cached in `~/.prompthub/layers/`.

Configure additional sources in `~/.prompthub/config.yaml`:

```yaml
sources:
  - name: official
    url: https://raw.githubusercontent.com/prompthub/layers/main
    default: true
  - name: my-team
    url: https://github.com/my-org/prompt-layers
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
# ✓ Logged in to registry.mycompany.internal (added as source 'registry.mycompany.internal')
#   Note: set 'default: true' in ~/.prompthub/config.yaml to use it by default.

# Interactive — prompts for username + password (calls POST /v1/auth/login)
ph login https://registry.mycompany.internal
# Username: alice
# Password: ********
# ✓ Logged in to my-company

# Remove token
ph logout https://registry.mycompany.internal
# ✓ Logged out from my-company
```

The token is stored in `~/.prompthub/config.yaml` (written with `0600` permissions):

```yaml
sources:
  - name: my-company
    url: https://registry.mycompany.internal
    default: true
    auth:
      token: phrt_xxxxxxxxxxxx
```

### Push a layer

```bash
# The layer must exist locally at layers/base/my-expert/v1.0/
ph push base/my-expert:v1.0
# ✓ Pushed base/my-expert:v1.0 to my-company

# Push to a specific named source
ph push --source my-company base/my-expert:v1.0

# Version immutability — pushing the same version twice is rejected
ph push base/my-expert:v1.0
# ✗ Version v1.0 already exists on my-company (versions are immutable)
```

`ph push` validates the layer locally before sending it, so bad layers are rejected before any network round-trip.

### Issue tokens (admin)

```bash
# Create a long-lived CI token (requires admin_token from registry.yaml)
curl -X POST https://registry.mycompany.internal/v1/auth/token \
  -H "Authorization: Bearer phrt_bootstrap_changeme" \
  -H "Content-Type: application/json" \
  -d '{"name": "ci-pipeline", "expires_in_days": 365}'
# {"token": "phrt_abc123...", "name": "ci-pipeline", "expires_at": "2027-03-16T..."}
```

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

## Project Layout

```
my-project/
  Promptfile              # Build description
  layers/                 # Project-private layers (not published)
    custom-role/
      layer.yaml
      prompt.md
  context.md              # Optional: included via INCLUDE
```

Global cache: `~/.prompthub/layers/`

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

### Setup

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

### Available Tools

| Tool | Description |
|------|-------------|
| `build_prompt` | Build a prompt from a Promptfile path or inline content, with optional `--var` overrides |
| `list_layers` | List all locally available layers (project + global cache) |
| `search_layers` | Search layers by keyword across name, description, and tags |
| `inspect_layer` | Show full metadata and prompt content for a specific layer |

### Example: Claude using PromptHub

Once configured, Claude can call these tools directly:

```
User: Build me a Chinese code review prompt for this PR.

Claude: [calls build_prompt with]
  content: |
    FROM base/code-reviewer:v1.0
    LAYER style/concise:v1.0
    VAR language "中文"
    TASK "Review the attached pull request."
  vars: ["language=中文"]

Result: [role] 你是一位资深代码审查专家... [constraints] 保持简洁...
        ---
        用中文审查这个 Pull Request。
```

## Works Well With

| Tool | How |
|------|-----|
| [Claude Code](https://github.com/anthropics/claude-code) | Use `ph-mcp` as an MCP server; define skill system prompts as Promptfiles |
| [Cursor](https://cursor.com) | Use `ph-mcp` as an MCP server |
| Any CI/CD pipeline | `ph build -o json` outputs structured prompt + model params |
| Private team registries | `ph push` / `ph pull` for versioned, shared layers |

## Other Commands

```bash
# Compare build output of two Promptfiles
ph diff Promptfile Promptfile.prod

# Show locally cached versions of a layer
ph history base/code-reviewer

# Search layers by keyword
ph search translation
```

## Real-World Validation

We rebuilt four skills from [anthropics/skills](https://github.com/anthropics/skills) using PromptHub to validate the layering approach. The exercise found three genuinely shared content blocks across the original skills:

| Shared Layer | Duplicated Across | What It Contains |
|---|---|---|
| `office-toolkit` | `docx`, `pptx`, `xlsx` | LibreOffice scripts, unpack/repack workflow |
| `office-quality` | `docx`, `xlsx` | Zero-error rule, Arial font, source documentation format |
| `anti-slop` | `frontend-design`, `pptx` | Anti-generic-AI-aesthetics design constraints |

The same content that lived in 3 separate skill files now lives in one layer. A single edit to `office-toolkit/prompt.md` propagates to all three Office skills on the next build.

### frontend-design

**Promptfile:**
```
FROM base/frontend-builder:v1.0
LAYER anti-slop:v1.0
TASK "Build the frontend interface described above."
```

**Built and executed:** Generated a PromptHub landing page. The `anti-slop` layer's constraints produced concrete choices: JetBrains Mono + Fraunces typefaces, near-black background with a single orange accent, asymmetric two-column hero layout, code syntax as the primary visual element.

### pptx

**Promptfile:**
```
FROM base/office-doc:v1.0
LAYER office-toolkit:v1.0
LAYER office-quality:v1.0
LAYER anti-slop:v1.0
TASK "Create or edit the PowerPoint presentation as described."
```

**Built and executed:** Generated a 3-slide PromptHub technical deck using pptxgenjs. The `anti-slop` layer produced a Midnight Executive palette (navy dominant, orange accent). The `office-toolkit` layer correctly guided tool selection to pptxgenjs for creation from scratch.

### xlsx

**Promptfile:**
```
FROM base/office-doc:v1.0
LAYER office-toolkit:v1.0
LAYER office-quality:v1.0
TASK "Create or edit the spreadsheet as described."
```

**Built and executed:** Generated a layer usage statistics workbook. The `office-quality` layer's constraints were applied precisely: hardcoded input values in blue (`INPUT_BLUE = "0000FF"`), formula results in black, all totals written as `=SUM(F5:F17)` rather than Python-computed values.

### What the rebuild revealed

Not all skills benefit from PromptHub. The `mcp-builder` skill's four-phase workflow (Research → Implement → Test → Evaluate) is a tightly coupled whole — splitting it into layers would break the logical flow. **PromptHub adds value where genuine shared content exists, not as a universal wrapper.**

## License

MIT
