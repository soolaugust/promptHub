# PromptHub

受 Docker 分层机制启发的 Prompt 管理系统。将可复用的 prompt 层组合成生产级 AI 指令。

[English](README.md)

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
║  TASK   "审查这个 Pull Request。"                ║
╚══════════════════════╦════════════════════════════╝
                       ║
                    ph build
                       ║
                       ▼
╔═══════════════════════════════════════════════════╗
║  合并后的 Prompt                                   ║
║                                                   ║
║  [role]          你是一位资深代码审查专家…         ║
║  [constraints]   保持简洁直接…                    ║
║  [constraints]   不输出敏感信息…                  ║
║  [output-format] ## 严重问题…                     ║
║                                                   ║
║  ---                                              ║
║  用中文审查这个 Pull Request。                     ║
╚═══════════════════════════════════════════════════╝
```

## 工作原理

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
  ║  [role]          ◀ 来自  code-reviewer       ║
  ║  [constraints]   ◀ 被  concise 覆盖          ║  ← 同名 section：后层优先
  ║  [constraints]   ◀ no-secrets 追加           ║  ← 新 section：直接追加
  ║  [output-format] ◀ 来自  code-reviewer       ║
  ║  ─────────────────────────────────────────── ║
  ║  审查代码                                     ║
  ╚══════════════════════════════════════════════╝
```

层合并规则确定：**同名 section → 后层覆盖前层**，**新 section → 追加**。变量（`${language}`）在构建时完成替换。

## 为什么选择 PromptHub

**对比手写 prompt 的三个优势：**
- **复用而非复制粘贴** — 一个层，多个 Promptfile。修复一处 bug，所有构建自动生效。
- **版本化、可审计** — `ph diff` 精确展示构建间的变化；`ph build -o json` 输出可复现的摘要。
- **团队共享** — 推送到私有 registry，团队成员拉取经过测试的精确版本。

**资源占用**（基于实际编译产物，实测数据）：

| | PromptHub | Python 方案 | Node 方案 |
|---|---|---|---|
| 二进制大小 | ~8 MB（`ph` + `ph-mcp`） | 50+ MB + 运行时 | 30+ MB + Node |
| 运行时依赖 | **无** | Python 3.x + pip | Node.js + npm |
| 安装方式 | `cargo install` 或直接复制二进制 | `pip install` | `npm install -g` |
| 冷启动 | **< 5 ms** | 200–500 ms | 100–300 ms |
| 空闲内存 | ~5 MB | ~30 MB | ~20 MB |

## 安装

```bash
cargo install --path prompthub/
```

安装后得到两个命令：
- `ph` — CLI 工具
- `ph-mcp` — 供 AI 助手（Claude、Cursor 等）使用的 MCP 服务器

如需同时安装私有 Registry 服务器：

```bash
cargo install --path registry/
```

## 快速开始

```bash
# 1. 从官方仓库拉取层
ph pull base/code-reviewer:v1.0

# 2. 创建 Promptfile
ph init

# 3. 构建并使用 prompt
ph build
```

```
$ ph build
[role]
你是一位资深代码审查专家，具备 10 年以上工程经验。

[constraints]
- 专注于逻辑错误和安全漏洞
- 提供具体的修复建议
...

$ ph build -o json
{
  "prompt": "[role]\nYou are a senior code reviewer...",
  "params": { "model": "claude-sonnet-4-6", "temperature": "0.3" },
  "layers": ["base/code-reviewer:v1.0", "style/concise:v1.0"],
  "digest": "sha256:a1b2c3..."
}
```

直接将 `ph build -o json` 的输出接入 CI 流水线或 API 调用 — 无需复制粘贴，不会产生版本漂移。

## Promptfile 语法

```
FROM base/code-reviewer:v1.0      # 基础层（必须，只能有一个，必须在第一行）
LAYER style/concise:latest         # 叠加层（可多个，按序合并）
LAYER guard/no-secrets:v1

VAR language "中文"                 # 声明变量（构建时可用 --var 覆盖）
PARAM model "claude-sonnet-4-6"    # 构建参数（会包含在 JSON 输出中）
PARAM temperature "0.3"

INCLUDE ./context.md               # 内联引入本地文件

TASK "用${language}审查这段代码"    # 任务描述（插入到 prompt 末尾）
```

| 指令 | 语法 | 说明 |
|------|------|------|
| `FROM` | `FROM <层>:<版本>` | 基础层，必须，首行，只能有一个 |
| `LAYER` | `LAYER <层>:<版本>` | 叠加层，可多个，按顺序合并 |
| `PARAM` | `PARAM <键> "<值>"` | 构建参数（model、temperature 等） |
| `VAR` | `VAR <名称> "<默认值>"` | 变量，构建时用 `--var 名称=值` 覆盖 |
| `TASK` | `TASK "<文本>"` | 任务描述，追加到 prompt 末尾 |
| `INCLUDE` | `INCLUDE <文件>` | 内联引入本地文件内容 |
| `#` | `# 注释` | 注释行 |

### 版本语法

| 写法 | 匹配规则 |
|------|---------|
| `layer:v1.0` | 精确版本 |
| `layer:v1` | v1.x 最新版 |
| `layer:latest` 或 `layer` | 最新可用版本 |

## Layer 规范

一个 Layer 是一个目录，包含两个文件：

```
layers/base/code-reviewer/
  layer.yaml       # 元数据
  prompt.md        # 带 [section] 标记的 prompt 内容
```

### layer.yaml

```yaml
name: code-reviewer
namespace: base
version: v1.0
description: "专业代码审查专家"
author: ph
tags: [code, review]
sections: [role, constraints, output-format]   # prompt.md 中定义的 section 列表
conflicts: [base/translator]                    # 互斥层
requires: []                                    # 依赖层
models: [claude-*, gpt-4*]                      # 兼容模型（glob 匹配）
```

### prompt.md

用 `[section-name]` 标记划分区域：

```markdown
[role]
你是一个资深代码审查专家，具备 10 年以上工程经验。

[constraints]
- 专注于逻辑错误和安全漏洞
- 提供具体的修复建议

[output-format]
## 问题列表
- **[严重程度]** `文件:行号` — 问题描述
## 总结
整体评价和建议。
```

### 合并规则

- **同名 section** → 后层覆盖前层（同时发出 warning）
- **新 section** → 追加到合并结果

## 官方种子层

| Layer | 说明 |
|-------|------|
| `base/code-reviewer` | 专业代码审查专家 |
| `base/translator` | 多语言翻译，注重文化适配 |
| `base/writer` | 清晰、有感染力的写作助手 |
| `base/analyst` | 严谨的数据分析专家 |
| `style/concise` | 简洁直接，控制在 300 词以内 |
| `style/verbose` | 详细解释，逐步推导 |
| `style/academic` | 学术写作风格 |
| `lang/chinese-markdown` | 简体中文 + Markdown 格式输出 |
| `lang/english-academic` | 英文学术格式 |
| `lang/structured-output` | 机器可解析的结构化输出 |
| `guard/no-secrets` | 禁止输出敏感信息和密钥 |
| `guard/safe-output` | 通用安全约束 |
| `guard/fact-check` | 强制事实准确性，标记不确定断言 |

## CLI 参考

```bash
# Layer 管理
ph layer new base/my-role          # 创建新 Layer 模板
ph layer list                      # 列出所有本地 Layer
ph layer inspect base/code-reviewer  # 查看元数据和内容
ph layer validate base/code-reviewer # 验证 Layer 格式

# 拉取和推送
ph pull base/code-reviewer:v1.0    # 从 registry 拉取（缓存至 ~/.prompthub/layers/）
ph push base/my-expert:v1.0        # 推送到 registry

# 构建与对比
ph build                           # 构建并输出到标准输出
ph build -o json                   # 结构化输出（prompt + 参数 + 摘要）
ph build --var language=English    # 覆盖变量
ph diff Promptfile Promptfile.prod # 对比两个 Promptfile

# 其他
ph search translation              # 按关键词搜索层
ph history base/code-reviewer      # 查看本地缓存版本历史
ph login --token <tok> <url>       # 认证到 registry
```

在 `~/.prompthub/config.yaml` 中配置自定义源：

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

## 私有 Registry

企业可以用 `ph-registry` 在内网自建私有层仓库——类似 Docker Registry，但专为 prompt 设计。支持 S3/MinIO 或本地文件系统存储，SQLite 元数据索引，token 认证。

```
  开发者 / CI 流水线 / AI Agent
                   │
                   │  HTTPS
                   ▼
  ╔══════════════════════════════════════════════════════╗
  ║  ph-registry  (Axum · Rust)                          ║
  ║                                                      ║
  ║  GET  /layers/{ns}/{name}/{ver}/layer.yaml           ║
  ║  GET  /layers/{ns}/{name}/{ver}/prompt.md            ║
  ║  PUT  /layers/{ns}/{name}/{ver}          (push)      ║
  ║  GET  /layers?q=keyword                  (搜索)      ║
  ║  POST /v1/auth/login                                 ║
  ║  POST /v1/auth/token                     (管理员)    ║
  ║                                                      ║
  ║  ┌─────────────────┐     ┌──────────────────────┐   ║
  ║  │   SQLite DB     │     │  S3 · MinIO · FS     │   ║
  ║  │  ▸ 用户         │     │  layers/             │   ║
  ║  │  ▸ token        │     │  └── {ns}/{name}/    │   ║
  ║  │  ▸ 层元数据     │     │      └── {ver}/      │   ║
  ║  └─────────────────┘     └──────────────────────┘   ║
  ╚══════════════════════════════════════════════════════╝
```

### 启动 Registry

**文件系统存储（单机，无外部依赖）：**

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

**Docker Compose（生产环境，配合 MinIO）：**

```bash
docker compose up
# 启动 ph-registry（:8080）+ MinIO（:9000）
```

完整配置见 [`registry/docker-compose.yaml`](registry/docker-compose.yaml)。

### 登录认证

```bash
# 非交互模式（CI 流水线 / AI Agent）——直接使用管理员签发的 token
ph login --token phrt_xxxxxxxxxxxx https://registry.mycompany.internal
# ✓ Logged in to registry.mycompany.internal

# 交互模式——输入用户名和密码
ph login https://registry.mycompany.internal

# 清除 token
ph logout https://registry.mycompany.internal
```

### 推送层

```bash
ph push base/my-expert:v1.0
# ✓ Pushed base/my-expert:v1.0 to my-company

# 推送到指定源
ph push --source my-company base/my-expert:v1.0

# 版本不可变——推送已存在的版本会被拒绝
```

`ph push` 在发送前会先在本地验证层的合法性，格式有误直接报错，不会浪费网络请求。

### 签发 token（管理员）

```bash
curl -X POST https://registry.mycompany.internal/v1/auth/token \
  -H "Authorization: Bearer phrt_bootstrap_changeme" \
  -H "Content-Type: application/json" \
  -d '{"name": "ci-pipeline", "expires_in_days": 365}'
# {"token": "phrt_abc123...", "name": "ci-pipeline", "expires_at": "2027-03-16T..."}
```

### Registry 工作流演示

![PromptHub registry demo](docs/demo-registry.gif)

### 完整工作流示例

```
# 1. 启动 Registry（运维一次性操作）
ph-registry registry.yaml

# 2. 认证（每台机器一次）
ph login --token phrt_abc123 https://registry.mycompany.internal

# 3. 在本地创建层
ph layer new base/sql-expert
# 编辑 layers/base/sql-expert/layer.yaml 和 prompt.md ...

# 4. 推送到私有 Registry
ph push base/sql-expert:v1.0
# ✓ Pushed base/sql-expert:v1.0 to my-company

# 5. 团队其他成员拉取
ph pull base/sql-expert:v1.0
# ✓ Pulled base/sql-expert:v1.0 to ~/.prompthub/layers/base/sql-expert/v1.0

# 6. 写入 Promptfile 使用
cat Promptfile
# FROM base/sql-expert:v1.0
# LAYER style/concise:v1.0
# TASK "优化以下 PostgreSQL 查询语句。"

ph build
```

## MCP 服务器

`ph-mcp` 是一个 MCP（Model Context Protocol）服务器，让 Claude、Cursor 等 AI 助手可以直接调用 PromptHub，无需手动复制粘贴。

```
  Claude Desktop / Cursor / Claude Code
           │
           │  MCP (stdio)
           ▼
  ╔═════════════════════════════════════════════╗
  ║  ph-mcp                                     ║
  ║                                             ║
  ║  build_prompt  ──▶  解析 → 解析层           ║
  ║                            ↓                ║
  ║                         合并 → 渲染         ║
  ║  list_layers   ──▶  扫描本地 + 全局缓存     ║
  ║  search_layers ──▶  按名称/描述/标签过滤    ║
  ║  inspect_layer ──▶  元数据 + 内容           ║
  ╚═════════════════════════════════════════════╝
           │
           ▼
  ~/.prompthub/layers/  +  ./layers/  (项目本地)
```

**Claude Desktop** — 添加到 `~/Library/Application Support/Claude/claude_desktop_config.json`：

```json
{
  "mcpServers": {
    "prompthub": {
      "command": "ph-mcp"
    }
  }
}
```

**Cursor** — 添加到项目的 `.cursor/mcp.json`：

```json
{
  "mcpServers": {
    "prompthub": {
      "command": "ph-mcp"
    }
  }
}
```

| 工具 | 说明 |
|------|------|
| `build_prompt` | 从 Promptfile 路径或内联内容构建 prompt，支持 `--var` 覆盖 |
| `list_layers` | 列出所有本地可用的层（项目层 + 全局缓存） |
| `search_layers` | 按关键词搜索层（匹配名称、描述、标签） |
| `inspect_layer` | 查看指定层的完整元数据和 prompt 内容 |

## 配合使用

| 工具 | 使用方式 |
|------|---------|
| [Claude Code](https://github.com/anthropics/claude-code) | 将 `ph-mcp` 作为 MCP 服务器；将技能系统提示定义为 Promptfile |
| [Cursor](https://cursor.com) | 将 `ph-mcp` 作为 MCP 服务器 |
| 任何 CI/CD 流水线 | `ph build -o json` 输出结构化 prompt + 模型参数 |
| 私有团队 Registry | `ph push` / `ph pull` 版本化共享层 |

## 真实场景验证

我们用 PromptHub 重构了 [anthropics/skills](https://github.com/anthropics/skills) 中的四个 skill，发现了三块真实共享内容：

| 共享层 | 原本重复分布于 | 层的内容 |
|--------|--------------|---------|
| `office-toolkit` | `docx`、`pptx`、`xlsx` | LibreOffice 脚本、解包/重打包工作流 |
| `office-quality` | `docx`、`xlsx` | 零错误规范、Arial 字体、来源注释格式 |
| `anti-slop` | `frontend-design`、`pptx` | 反通用 AI 审美设计约束 |

原本分散在三个 skill 文件里的内容，现在统一维护在一个层里。修改 `office-toolkit/prompt.md` 一处，所有引用它的 skill 下次构建时自动生效。

并非所有 skill 都适合用 PromptHub 管理。`mcp-builder` 的四阶段工作流（研究 → 实现 → 测试 → 评估）是紧密耦合的整体——强行拆层会破坏流程逻辑。**PromptHub 在存在真实共享内容时才有价值，不是通用包装器。**

## License

MIT
