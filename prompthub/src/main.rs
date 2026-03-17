mod cli;
mod export;

use prompthub::{config, layer, merger, output, parser, pull, push, renderer, resolver};
use anyhow::Context;
use clap::Parser;
use cli::{Cli, Commands, LayerCommands};
use colored::Colorize;
use std::path::{Path, PathBuf};

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("{} {}", "error:".red().bold(), e);
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    config::ensure_dirs()?;

    match cli.command {
        Commands::Init { dir } => cmd_init(&dir),
        Commands::Build { promptfile, var, output, warn } => {
            cmd_build(&promptfile, &var, &output, warn)
        }
        Commands::Layer(layer_cmd) => match layer_cmd {
            LayerCommands::New { name, dir } => cmd_layer_new(&name, &dir),
            LayerCommands::List { namespace } => cmd_layer_list(namespace.as_deref()),
            LayerCommands::Inspect { name } => cmd_layer_inspect(&name),
            LayerCommands::Validate { name } => cmd_layer_validate(&name),
        },
        Commands::Pull { layer } => cmd_pull(&layer),
        Commands::Search { keyword } => cmd_search(&keyword),
        Commands::Diff { first, second } => cmd_diff(&first, &second),
        Commands::History { layer } => cmd_history(&layer),
        Commands::Login { registry_url, token } => cmd_login(&registry_url, token.as_deref()),
        Commands::Logout { registry_url } => cmd_logout(&registry_url),
        Commands::Push { layer, source } => cmd_push(&layer, source.as_deref()),
        Commands::Export { source, name, output, no_analyze, refactor, yes } => {
            export::cmd_export(&source, name.as_deref(), &output, no_analyze, refactor, yes)
        }
    }
}

// ── init ─────────────────────────────────────────────────────────────────────

fn cmd_init(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let promptfile_path = dir.join("Promptfile");

    if promptfile_path.exists() {
        anyhow::bail!("Promptfile already exists at {}", promptfile_path.display());
    }

    let template = r#"# PromptHub Promptfile
# Docs: https://github.com/prompthub/prompthub

FROM base/code-reviewer:v1.0      # Base layer (required, must be first)
LAYER style/concise:v1.0          # Style layer
# LAYER guard/no-secrets:v1.0     # Uncomment to add safety constraints

VAR language "English"            # Override with: prompthub build --var language=中文
PARAM model "claude-sonnet-4-6"
PARAM temperature "0.3"

# INCLUDE ./context.md            # Uncomment to include additional context

TASK "Review the following code and provide feedback."
"#;

    std::fs::write(&promptfile_path, template)?;
    println!("{} Created {}", "✓".green(), promptfile_path.display());
    println!("\nNext steps:");
    println!("  prompthub layer list           # see available layers");
    println!("  prompthub build                # build your prompt");
    println!("  prompthub pull base/writer     # fetch more layers");
    Ok(())
}

// ── build ─────────────────────────────────────────────────────────────────────

fn cmd_build(
    promptfile_path: &Path,
    var_overrides: &[String],
    format: &output::OutputFormat,
    show_warnings: bool,
) -> anyhow::Result<()> {
    let base_dir = promptfile_path.parent().unwrap_or(Path::new("."));

    let content = std::fs::read_to_string(promptfile_path)
        .with_context(|| format!("Cannot read Promptfile: {}", promptfile_path.display()))?;
    let mut pf = parser::parse(&content).map_err(|e| anyhow::anyhow!("{}", e))?;

    for var_str in var_overrides {
        let (name, value) = parser::parse_var_override(var_str)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        pf.vars.insert(name, value);
    }

    let local_layers = base_dir.join("layers");
    let resolver = resolver::LayerResolver::new(vec![local_layers]);

    let base_layer = resolver.resolve(&pf.from).map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut additional_layers = Vec::new();
    for layer_ref in &pf.layers {
        let l = resolver.resolve(layer_ref).map_err(|e| anyhow::anyhow!("{}", e))?;
        additional_layers.push(l);
    }

    let mut include_contents = Vec::new();
    for include_path in &pf.includes {
        let c = renderer::load_include(include_path, base_dir)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        include_contents.push((include_path.clone(), c));
    }

    let merged = merger::merge_layers(&base_layer, &additional_layers, pf.params.clone())
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if show_warnings {
        for w in &merged.warnings {
            eprintln!("{} {}", "warning:".yellow(), w);
        }
    }

    let (final_text, undef_vars) = renderer::render_variables(
        &merged,
        &pf.vars,
        pf.task.as_deref(),
        &include_contents,
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Surface undefined-variable warnings through the same channel as merge warnings
    let mut all_warnings = merged.warnings.clone();
    for var_name in &undef_vars {
        let msg = format!("Undefined variable: ${{{}}}", var_name);
        if show_warnings {
            eprintln!("{} {}", "warning:".yellow(), msg);
        }
        all_warnings.push(msg);
    }

    let layer_names: Vec<String> = std::iter::once(pf.from.display())
        .chain(pf.layers.iter().map(|r| r.display()))
        .collect();

    output::output_result(&final_text, format, &merged.params, &layer_names, &all_warnings)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok(())
}

// ── layer new ─────────────────────────────────────────────────────────────────

fn cmd_layer_new(name: &str, base_dir: &Path) -> anyhow::Result<()> {
    let (namespace, layer_name) = parse_layer_name(name);
    let layer_dir = base_dir.join(&namespace).join(&layer_name);

    if layer_dir.exists() {
        anyhow::bail!("Layer directory already exists: {}", layer_dir.display());
    }
    std::fs::create_dir_all(&layer_dir)?;

    // Build the LayerMeta struct and serialize it so the output is always
    // valid YAML regardless of special characters in the layer name.
    let template_meta = layer::LayerMeta {
        name: layer_name.clone(),
        namespace: namespace.clone(),
        version: "v1.0".to_string(),
        description: String::new(),
        author: String::new(),
        tags: Vec::new(),
        sections: vec!["role".to_string(), "constraints".to_string(), "output-format".to_string()],
        conflicts: Vec::new(),
        requires: Vec::new(),
        models: vec!["claude-*".to_string(), "gpt-4*".to_string()],
    };
    let yaml = serde_yaml::to_string(&template_meta)
        .with_context(|| "Failed to serialize layer.yaml template")?;

    let prompt = "[role]\nDescribe the role or persona here.\n\n[constraints]\n- Constraint 1\n- Constraint 2\n\n[output-format]\nDescribe the expected output format.\n";

    std::fs::write(layer_dir.join("layer.yaml"), yaml)?;
    std::fs::write(layer_dir.join("prompt.md"), prompt)?;

    println!("{} Created layer: {}/{}", "✓".green(), namespace, layer_name);
    println!("  {}", layer_dir.join("layer.yaml").display());
    println!("  {}", layer_dir.join("prompt.md").display());
    Ok(())
}

// ── layer list ────────────────────────────────────────────────────────────────

fn cmd_layer_list(namespace_filter: Option<&str>) -> anyhow::Result<()> {
    let mut all_layers: Vec<(String, PathBuf)> = Vec::new();

    let local = PathBuf::from("layers");
    if local.exists() {
        for (name, path) in resolver::scan_layers(&local) {
            all_layers.push((format!("{} (local)", name), path));
        }
    }

    let global = config::global_layers_dir();
    all_layers.extend(resolver::scan_layers(&global));

    if all_layers.is_empty() {
        println!("No layers found. Run `prompthub pull <layer>` to fetch layers.");
        return Ok(());
    }

    let filtered: Vec<_> = all_layers
        .iter()
        .filter(|(name, _)| {
            namespace_filter.map(|ns| name.starts_with(ns)).unwrap_or(true)
        })
        .collect();

    // Compute column width dynamically so long names don't truncate
    let name_width = filtered.iter()
        .map(|(n, _)| n.len())
        .max()
        .unwrap_or(5)
        .max(5); // at least as wide as "LAYER"
    let separator_width = name_width + 2 + 40; // name + gap + path column
    println!("{:<width$} {}", "LAYER".bold(), "PATH".bold(), width = name_width);
    println!("{}", "-".repeat(separator_width));
    for (name, path) in &filtered {
        println!("{:<width$} {}", name, path.display(), width = name_width);
    }
    println!("\n{} layer(s) found", filtered.len());
    Ok(())
}

// ── layer inspect ─────────────────────────────────────────────────────────────

fn cmd_layer_inspect(name: &str) -> anyhow::Result<()> {
    let l = find_layer(name)?;

    println!("{}", "─".repeat(60));
    println!("{}: {}/{}", "Name".bold(), l.meta.namespace, l.meta.name);
    println!("{}: {}", "Version".bold(), l.meta.version);
    println!("{}: {}", "Description".bold(), l.meta.description);
    println!("{}: {}", "Author".bold(), l.meta.author);
    println!("{}: {}", "Tags".bold(), l.meta.tags.join(", "));
    println!("{}: {}", "Sections".bold(), l.meta.sections.join(", "));
    if !l.meta.conflicts.is_empty() {
        println!("{}: {}", "Conflicts".bold(), l.meta.conflicts.join(", "));
    }
    if !l.meta.models.is_empty() {
        println!("{}: {}", "Models".bold(), l.meta.models.join(", "));
    }
    println!("{}", "─".repeat(60));
    println!("{}", "Content:".bold());
    println!();
    println!("{}", l.content);
    Ok(())
}

// ── layer validate ────────────────────────────────────────────────────────────

fn cmd_layer_validate(name: &str) -> anyhow::Result<()> {
    let l = find_layer(name)?;
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Note: name and version emptiness are already enforced by Layer::load_from_dir
    // (which returns a ValidationError before reaching this point), so those checks
    // are not duplicated here.

    if l.meta.namespace.is_empty() {
        warnings.push("namespace is empty (layers should have a namespace for proper resolution)".to_string());
    }
    if l.meta.description.is_empty() {
        warnings.push("description is empty".to_string());
    }
    if l.content.is_empty() {
        errors.push("prompt.md is empty".to_string());
    }
    for section in &l.meta.sections {
        if !l.sections.contains_key(section) {
            warnings.push(format!("declared section '{}' not found in prompt.md", section));
        }
    }

    for e in &errors {
        println!("{} {}", "✗".red(), e);
    }
    for w in &warnings {
        println!("{} {}", "⚠".yellow(), w);
    }

    if errors.is_empty() {
        println!("{} Layer '{}' is valid", "✓".green(), name);
        Ok(())
    } else {
        anyhow::bail!("Validation failed with {} error(s)", errors.len())
    }
}

// ── pull ──────────────────────────────────────────────────────────────────────

fn cmd_pull(layer_str: &str) -> anyhow::Result<()> {
    let layer_ref = parser::LayerRef::parse(layer_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    let config = config::Config::load()?;
    pull::pull_layer(&layer_ref, &config).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(())
}

// ── search ────────────────────────────────────────────────────────────────────

fn cmd_search(keyword: &str) -> anyhow::Result<()> {
    let results = resolver::search_layers(
        &[PathBuf::from("layers"), config::global_layers_dir()],
        keyword,
    );

    if results.is_empty() {
        println!("No layers found for '{}'", keyword);
        return Ok(());
    }

    println!("{:<40} {:<10} {}", "LAYER".bold(), "VERSION".bold(), "DESCRIPTION".bold());
    println!("{}", "-".repeat(80));
    for (name, l) in &results {
        println!("{:<40} {:<10} {}", name, l.meta.version, l.meta.description);
    }
    Ok(())
}

// ── diff ──────────────────────────────────────────────────────────────────────

fn cmd_diff(first: &Path, second: &Path) -> anyhow::Result<()> {
    let text1 = build_to_text(first)?;
    let text2 = build_to_text(second)?;

    if text1 == text2 {
        println!("No differences found.");
        return Ok(());
    }

    let lines1: Vec<&str> = text1.lines().collect();
    let lines2: Vec<&str> = text2.lines().collect();

    println!("{}", format!("--- {}", first.display()).red());
    println!("{}", format!("+++ {}", second.display()).green());

    let max = lines1.len().max(lines2.len());
    for i in 0..max {
        match (lines1.get(i), lines2.get(i)) {
            (Some(a), Some(b)) if a == b => println!("  {}", a),
            (Some(a), Some(b)) => {
                println!("{}", format!("- {}", a).red());
                println!("{}", format!("+ {}", b).green());
            }
            (Some(a), None) => println!("{}", format!("- {}", a).red()),
            (None, Some(b)) => println!("{}", format!("+ {}", b).green()),
            (None, None) => {}
        }
    }
    Ok(())
}

fn build_to_text(promptfile_path: &Path) -> anyhow::Result<String> {
    let base_dir = promptfile_path.parent().unwrap_or(Path::new("."));
    let content = std::fs::read_to_string(promptfile_path)
        .with_context(|| format!("Cannot read {}", promptfile_path.display()))?;
    let pf = parser::parse(&content).map_err(|e| anyhow::anyhow!("{}", e))?;

    let resolver = resolver::LayerResolver::new(vec![base_dir.join("layers")]);
    let base_layer = resolver.resolve(&pf.from).map_err(|e| anyhow::anyhow!("{}", e))?;
    let additional: anyhow::Result<Vec<_>> = pf
        .layers
        .iter()
        .map(|r| resolver.resolve(r).map_err(|e| anyhow::anyhow!("{}", e)))
        .collect();
    let merged = merger::merge_layers(&base_layer, &additional?, pf.params.clone())
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    renderer::render_variables(&merged, &pf.vars, pf.task.as_deref(), &[])
        .map(|(text, _)| text)
        .map_err(|e| anyhow::anyhow!("{}", e))
}

// ── history ───────────────────────────────────────────────────────────────────

fn cmd_history(layer_name: &str) -> anyhow::Result<()> {
    let layer_path = config::global_layers_dir().join(layer_name);

    if !layer_path.exists() {
        println!("Layer '{}' not found in local cache.", layer_name);
        return Ok(());
    }

    let mut versions: Vec<String> = std::fs::read_dir(&layer_path)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    // Use semver-aware sort so v1.10 appears after v1.9
    versions.sort_by(|a, b| {
        match (resolver::parse_semver(a), resolver::parse_semver(b)) {
            (Some(va), Some(vb)) => va.cmp(&vb),
            _ => a.cmp(b),
        }
    });

    println!("{}", format!("Versions of '{}':", layer_name).bold());
    for v in &versions {
        println!("  {}", v);
    }
    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn parse_layer_name(name: &str) -> (String, String) {
    let parts: Vec<&str> = name.splitn(2, '/').collect();
    if parts.len() == 2 {
        (parts[0].to_string(), parts[1].to_string())
    } else {
        ("custom".to_string(), parts[0].to_string())
    }
}

fn find_layer(name: &str) -> anyhow::Result<layer::Layer> {
    let layer_ref = parser::LayerRef::parse(name).map_err(|e| anyhow::anyhow!("{}", e))?;
    // Search both project-local and global layers directory
    let resolver = resolver::LayerResolver::new(vec![
        PathBuf::from("layers"),
        config::global_layers_dir(),
    ]);
    resolver.resolve(&layer_ref).map_err(|e| anyhow::anyhow!("{}", e))
}

// ── login ─────────────────────────────────────────────────────────────────────

fn cmd_login(registry_url: &str, token_flag: Option<&str>) -> anyhow::Result<()> {
    let token = if let Some(t) = token_flag {
        // Non-interactive: token provided via --token flag
        t.to_string()
    } else {
        // Interactive: prompt for username + password, call POST /v1/auth/login
        let username = {
            print!("Username: ");
            use std::io::{self, Write};
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            input.trim().to_string()
        };
        let password = rpassword::prompt_password("Password: ")?;

        // Call POST /v1/auth/login
        let url = format!("{}/v1/auth/login", registry_url.trim_end_matches('/'));
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        let response = client
            .post(&url)
            .json(&serde_json::json!({"username": username, "password": password}))
            .send()
            .map_err(|e| anyhow::anyhow!("Cannot reach registry: {}", e))?;

        if !response.status().is_success() {
            anyhow::bail!("Login failed: invalid credentials");
        }

        let body: serde_json::Value = response.json()?;
        body["token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Registry returned no token"))?
            .to_string()
    };

    // Update config
    let mut cfg = config::Config::load()?;

    if let Some(src) = cfg.find_source_by_url_mut(registry_url) {
        src.auth = Some(config::SourceAuth { token });
        let name = src.name.clone();
        cfg.save()?;
        println!("{} Logged in to {}", "✓".green(), name);
    } else {
        // No matching source — create a new entry from hostname
        let hostname = registry_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or(registry_url)
            .to_string();
        let new_source = config::Source {
            name: hostname.clone(),
            url: registry_url.trim_end_matches('/').to_string(),
            default: false,
            auth: Some(config::SourceAuth { token }),
        };
        cfg.sources.push(new_source);
        cfg.save()?;
        println!("{} Logged in to {} (added as source '{}')", "✓".green(), registry_url, hostname);
        println!("  Note: set 'default: true' in ~/.prompthub/config.yaml to use it by default.");
    }

    Ok(())
}

// ── logout ────────────────────────────────────────────────────────────────────

fn cmd_logout(registry_url: &str) -> anyhow::Result<()> {
    let mut cfg = config::Config::load()?;

    if let Some(src) = cfg.find_source_by_url_mut(registry_url) {
        let name = src.name.clone();
        src.auth = None;
        cfg.save()?;
        println!("{} Logged out from {}", "✓".green(), name);
    } else {
        println!("No source configured for {}", registry_url);
    }

    Ok(())
}

// ── push ──────────────────────────────────────────────────────────────────────

fn cmd_push(layer_str: &str, source_name: Option<&str>) -> anyhow::Result<()> {
    let target = push::PushTarget::parse(layer_str)?;
    let cfg = config::Config::load()?;

    let source = if let Some(name) = source_name {
        cfg.sources.iter().find(|s| s.name == name)
            .ok_or_else(|| anyhow::anyhow!("Source '{}' not found in config", name))?
    } else {
        cfg.default_source()
            .ok_or_else(|| anyhow::anyhow!("No default source configured"))?
    };

    let layers_dir = PathBuf::from("layers");

    match push::push_layer(&target, source, &layers_dir)? {
        push::PushResult::Success(source_name) => {
            println!(
                "{} Pushed {}/{}:{} to {}",
                "✓".green(),
                target.namespace, target.name, target.version,
                source_name
            );
        }
        push::PushResult::AlreadyExists(source_name) => {
            eprintln!(
                "{} Version {} already exists on {} (versions are immutable)",
                "✗".red(),
                target.version,
                source_name
            );
            std::process::exit(1);
        }
    }

    Ok(())
}
