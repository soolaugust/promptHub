//! PromptHub MCP Server
//!
//! Exposes PromptHub capabilities as MCP tools so AI assistants can
//! build, search, list and inspect prompt layers via the Model Context Protocol.
//!
//! Run with:
//!   ph-mcp
//!
//! Then register in your MCP client config (e.g. Claude Desktop):
//!   { "command": "ph-mcp" }

use anyhow::Context as _;
use prompthub::{config, merger, parser, renderer, resolver};
use rmcp::{
    handler::server::{
        tool::ToolRouter,
        wrapper::Parameters,
    },
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
    ServiceExt,
};
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// ── Parameter structs ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
struct BuildPromptParams {
    /// Inline Promptfile content (e.g. "FROM base/code-reviewer:v1.0\nTASK \"review code\"").
    /// Mutually exclusive with `promptfile_path`.
    #[serde(default)]
    promptfile_content: Option<String>,

    /// Absolute path to a Promptfile on disk.
    /// Mutually exclusive with `promptfile_content`.
    #[serde(default)]
    promptfile_path: Option<String>,

    /// Variable overrides in "KEY=VALUE" format (e.g. ["language=中文"]).
    #[serde(default)]
    vars: Vec<String>,

    /// Extra layer search path (absolute directory).
    /// Defaults to the current working directory's layers/ subdirectory.
    #[serde(default)]
    layers_dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchLayersParams {
    /// Keyword to search for in layer names, descriptions and tags.
    keyword: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct InspectLayerParams {
    /// Layer reference, e.g. "base/code-reviewer" or "base/code-reviewer:v1.0".
    layer_ref: String,

    /// Extra layer search path (absolute directory). Optional.
    #[serde(default)]
    layers_dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListLayersParams {
    /// Optional namespace filter, e.g. "base" or "style".
    #[serde(default)]
    namespace: Option<String>,
}

// ── Server struct ────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct PromptHubServer {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl PromptHubServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    /// Build a prompt from a Promptfile (inline content or path) and return the rendered text.
    #[tool(description = "Build a PromptHub prompt by merging layers defined in a Promptfile. \
        Supply either `promptfile_content` (inline Promptfile text) or `promptfile_path` (absolute \
        path to a file on disk). Variable overrides are passed as [\"KEY=VALUE\"] strings in `vars`. \
        Returns JSON with the fully rendered prompt text plus params, layers used, and warnings.")]
    async fn build_prompt(
        &self,
        Parameters(p): Parameters<BuildPromptParams>,
    ) -> Result<String, String> {
        build_prompt_impl(p).map_err(|e| e.to_string())
    }

    /// List all locally available layers.
    #[tool(description = "List all PromptHub layers available in the local cache (~/.prompthub/layers) \
        and optionally the current project's layers/ directory. \
        Optionally filter by namespace (e.g. 'base', 'style', 'guard'). \
        Returns JSON array of {name, path} objects.")]
    async fn list_layers(
        &self,
        Parameters(p): Parameters<ListLayersParams>,
    ) -> Result<String, String> {
        list_layers_impl(p).map_err(|e| e.to_string())
    }

    /// Search layers by keyword.
    #[tool(description = "Search locally cached PromptHub layers by keyword. \
        Matches against layer name, description and tags. \
        Returns JSON array of matching layers with name, version, description and tags.")]
    async fn search_layers(
        &self,
        Parameters(p): Parameters<SearchLayersParams>,
    ) -> Result<String, String> {
        search_layers_impl(p).map_err(|e| e.to_string())
    }

    /// Inspect a specific layer: metadata + raw prompt content.
    #[tool(description = "Inspect a PromptHub layer: show its metadata (name, version, description, \
        author, tags, sections, conflicts, models) and the full prompt.md content. \
        layer_ref format: 'namespace/name' or 'namespace/name:version'.")]
    async fn inspect_layer(
        &self,
        Parameters(p): Parameters<InspectLayerParams>,
    ) -> Result<String, String> {
        inspect_layer_impl(p).map_err(|e| e.to_string())
    }
}

#[tool_handler]
impl rmcp::ServerHandler for PromptHubServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "PromptHub MCP server: build, search, list and inspect layered prompt templates. \
                 Use build_prompt to render a Promptfile into a final prompt. \
                 Use list_layers / search_layers to discover available layers. \
                 Use inspect_layer to read a layer's full content and metadata.",
            )
    }
}

// ── Implementation helpers ───────────────────────────────────────────────────

fn build_prompt_impl(params: BuildPromptParams) -> anyhow::Result<String> {
    let (content, base_dir) = match (params.promptfile_content, params.promptfile_path) {
        (Some(inline), _) => (inline, PathBuf::from(".")),
        (None, Some(path)) => {
            let p = PathBuf::from(&path);
            let base = p.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
            let text = std::fs::read_to_string(&p)
                .with_context(|| format!("Cannot read Promptfile: {}", p.display()))?;
            (text, base)
        }
        (None, None) => anyhow::bail!(
            "Provide either `promptfile_content` (inline text) or `promptfile_path` (file path)"
        ),
    };

    let mut pf = parser::parse(&content).map_err(|e| anyhow::anyhow!("{}", e))?;

    for var_str in &params.vars {
        let parts: Vec<&str> = var_str.splitn(2, '=').collect();
        if parts.len() != 2 {
            anyhow::bail!("Invalid var format '{}'. Use NAME=VALUE", var_str);
        }
        pf.vars.insert(parts[0].to_string(), parts[1].to_string());
    }

    let mut search_paths = vec![base_dir.join("layers")];
    if let Some(extra) = params.layers_dir {
        search_paths.push(PathBuf::from(extra));
    }
    let res = resolver::LayerResolver::new(search_paths);

    let base_layer = res.resolve(&pf.from).map_err(|e| anyhow::anyhow!("{}", e))?;
    let mut additional_layers = Vec::new();
    for lr in &pf.layers {
        additional_layers.push(res.resolve(lr).map_err(|e| anyhow::anyhow!("{}", e))?);
    }

    let mut include_contents = Vec::new();
    for inc in &pf.includes {
        let c = renderer::load_include(inc, &base_dir)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        include_contents.push((inc.clone(), c));
    }

    let merged = merger::merge_layers(&base_layer, &additional_layers, pf.params.clone())
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let (final_text, undef_vars) = renderer::render_variables(
        &merged,
        &pf.vars,
        pf.task.as_deref(),
        &include_contents,
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    let layer_names: Vec<String> = std::iter::once(pf.from.display())
        .chain(pf.layers.iter().map(|r| r.display()))
        .collect();

    let mut all_warnings = merged.warnings.clone();
    for var_name in &undef_vars {
        all_warnings.push(format!("Undefined variable: ${{{}}}", var_name));
    }

    #[derive(Serialize)]
    struct BuildResult<'a> {
        prompt: &'a str,
        params: &'a HashMap<String, String>,
        layers: Vec<String>,
        warnings: Vec<String>,
    }

    let out = BuildResult {
        prompt: &final_text,
        params: &merged.params,
        layers: layer_names,
        warnings: all_warnings,
    };

    Ok(serde_json::to_string_pretty(&out)?)
}

fn list_layers_impl(params: ListLayersParams) -> anyhow::Result<String> {
    let mut all: Vec<(String, PathBuf)> = Vec::new();

    let local = PathBuf::from("layers");
    if local.exists() {
        for (name, path) in resolver::scan_layers(&local) {
            all.push((format!("{} (local)", name), path));
        }
    }
    all.extend(resolver::scan_layers(&config::global_layers_dir()));

    if all.is_empty() {
        return Ok(r#"{"layers":[],"message":"No layers found. Use `ph pull <layer>` to fetch layers."}"#.to_string());
    }

    let filtered: Vec<_> = all
        .iter()
        .filter(|(name, _)| {
            params
                .namespace
                .as_deref()
                .map(|ns| name.starts_with(ns))
                .unwrap_or(true)
        })
        .collect();

    #[derive(Serialize)]
    struct LayerEntry<'a> {
        name: &'a str,
        path: String,
    }

    let entries: Vec<LayerEntry> = filtered
        .iter()
        .map(|(name, path)| LayerEntry {
            name: name.as_str(),
            path: path.display().to_string(),
        })
        .collect();

    Ok(serde_json::to_string_pretty(&entries)?)
}

fn search_layers_impl(params: SearchLayersParams) -> anyhow::Result<String> {
    let results = resolver::search_layers(
        &[PathBuf::from("layers"), config::global_layers_dir()],
        &params.keyword,
    );

    if results.is_empty() {
        return Ok(format!(r#"{{"results":[],"message":"No layers found for '{}'}}"#, params.keyword));
    }

    #[derive(Serialize)]
    struct SearchEntry {
        name: String,
        version: String,
        description: String,
        tags: Vec<String>,
    }

    let entries: Vec<SearchEntry> = results
        .into_iter()
        .map(|(name, l)| SearchEntry {
            name,
            version: l.meta.version,
            description: l.meta.description,
            tags: l.meta.tags,
        })
        .collect();

    Ok(serde_json::to_string_pretty(&entries)?)
}

fn inspect_layer_impl(params: InspectLayerParams) -> anyhow::Result<String> {
    let layer_ref = parser::LayerRef::parse(&params.layer_ref)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut search_paths = vec![PathBuf::from("layers"), config::global_layers_dir()];
    if let Some(extra) = params.layers_dir {
        search_paths.push(PathBuf::from(extra));
    }
    let res = resolver::LayerResolver::new(search_paths);
    let l = res.resolve(&layer_ref).map_err(|e| anyhow::anyhow!("{}", e))?;

    #[derive(Serialize)]
    struct InspectResult {
        name: String,
        namespace: String,
        version: String,
        description: String,
        author: String,
        tags: Vec<String>,
        sections: Vec<String>,
        conflicts: Vec<String>,
        requires: Vec<String>,
        models: Vec<String>,
        content: String,
    }

    let result = InspectResult {
        name: l.meta.name,
        namespace: l.meta.namespace,
        version: l.meta.version,
        description: l.meta.description,
        author: l.meta.author,
        tags: l.meta.tags,
        sections: l.meta.sections,
        conflicts: l.meta.conflicts,
        requires: l.meta.requires,
        models: l.meta.models,
        content: l.content,
    };

    Ok(serde_json::to_string_pretty(&result)?)
}

// ── Entry point ──────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Log to stderr so stdout stays clean for MCP JSON-RPC
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ph_mcp=info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    config::ensure_dirs()?;

    tracing::info!("Starting PromptHub MCP server");

    let server = PromptHubServer::new();
    let service = server
        .serve(stdio())
        .await
        .context("Failed to start MCP server")?;

    service.waiting().await?;
    Ok(())
}
