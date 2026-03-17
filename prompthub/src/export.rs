use std::path::{Path, PathBuf};
use anyhow::Context;
use colored::Colorize;
use prompthub::layer::{LayerMeta, parse_sections};

// ── public entry point ────────────────────────────────────────────────────────

pub fn cmd_export(
    source: &crate::cli::ExportSource,
    name: Option<&str>,
    output: &str,
    no_analyze: bool,
    refactor: bool,
    yes: bool,
) -> anyhow::Result<()> {
    let output_path = Path::new(output);
    match source {
        crate::cli::ExportSource::Skills => {
            export_skills(name, output_path)?;
            if name.is_none() {
                if refactor {
                    run_refactor(output_path, yes)?;
                } else if !no_analyze {
                    run_similarity_analysis(output_path)?;
                }
            }
            Ok(())
        }
        crate::cli::ExportSource::Layers => export_layers(name, output_path),
    }
}

// ── skill export ──────────────────────────────────────────────────────────────

fn export_skills(name: Option<&str>, output: &Path) -> anyhow::Result<()> {
    let skills_dir = skills_base_dir();

    if !skills_dir.exists() {
        anyhow::bail!(
            "Skills directory not found: {}",
            skills_dir.display()
        );
    }

    let skill_entries = collect_skill_dirs(&skills_dir, name)?;

    if skill_entries.is_empty() {
        if let Some(n) = name {
            anyhow::bail!("Skill '{}' not found in {}", n, skills_dir.display());
        } else {
            println!("No skills found in {}", skills_dir.display());
            return Ok(());
        }
    }

    let mut count = 0usize;
    for (skill_name, skill_dir) in &skill_entries {
        match export_one_skill(skill_name, skill_dir, output) {
            Ok(dest) => {
                println!("{} Exported {} → {}/", "✓".green(), skill_name, dest.display());
                count += 1;
            }
            Err(e) => {
                eprintln!("{} Skipping '{}': {}", "⚠".yellow(), skill_name, e);
            }
        }
    }

    println!("  {} skill(s) exported to {}/", count, output.display());
    Ok(())
}

fn collect_skill_dirs(
    skills_dir: &Path,
    name_filter: Option<&str>,
) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut results = Vec::new();

    if let Some(n) = name_filter {
        // Single skill requested
        let skill_dir = skills_dir.join(n);
        if skill_dir.is_dir() {
            results.push((n.to_string(), skill_dir));
        }
        return Ok(results);
    }

    // All skills
    let entries = std::fs::read_dir(skills_dir)
        .with_context(|| format!("Cannot read skills directory: {}", skills_dir.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                results.push((dir_name.to_string(), path));
            }
        }
    }

    results.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(results)
}

fn export_one_skill(skill_name: &str, skill_dir: &Path, output: &Path) -> anyhow::Result<PathBuf> {
    let skill_md_path = skill_dir.join("SKILL.md");
    if !skill_md_path.exists() {
        anyhow::bail!("SKILL.md not found in {}", skill_dir.display());
    }

    let content = std::fs::read_to_string(&skill_md_path)
        .with_context(|| format!("Cannot read {}", skill_md_path.display()))?;

    let (fm, body) = parse_skill_md(&content)
        .with_context(|| format!("Cannot parse SKILL.md for '{}'", skill_name))?;

    // Use name from frontmatter if present, otherwise use directory name
    let layer_name = if fm.name.is_empty() {
        skill_name.to_string()
    } else {
        fm.name.clone()
    };

    // Determine git author for the author field
    let author = git_user_name().unwrap_or_default();

    // Build tags: add argument-hint as a tag if present
    let mut tags = Vec::new();
    if let Some(hint) = &fm.argument_hint {
        if !hint.is_empty() {
            tags.push(hint.clone());
        }
    }

    // Detect sections in the body
    let (parsed_sections, _) = parse_sections(&body);
    let sections: Vec<String> = {
        // Preserve declaration order by scanning the body text for headers
        let mut ordered = Vec::new();
        for line in body.lines() {
            // Reuse the same simple detection: [word] at start of line
            let trimmed = line.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() > 2 {
                let inner = &trimmed[1..trimmed.len() - 1];
                let lower = inner.to_lowercase();
                if parsed_sections.contains_key(&lower) && !ordered.contains(&lower) {
                    ordered.push(lower);
                }
            }
        }
        ordered
    };

    // Build the prompt.md content
    // If no sections found, wrap content under [instructions]
    let prompt_content = if sections.is_empty() {
        format!("[instructions]\n{}\n", body.trim_end())
    } else {
        body.clone()
    };

    // sections field in layer.yaml: either detected sections or ["instructions"]
    let yaml_sections = if sections.is_empty() {
        vec!["instructions".to_string()]
    } else {
        sections
    };

    let meta = LayerMeta {
        name: layer_name.clone(),
        namespace: "skill".to_string(),
        version: "v1.0".to_string(),
        description: fm.description,
        author,
        tags,
        sections: yaml_sections,
        conflicts: Vec::new(),
        requires: Vec::new(),
        models: Vec::new(),
        language: None,
        family: None,
    };

    // Output path: {output}/skill/{name}/v1.0/
    let dest = output.join("skill").join(&layer_name).join("v1.0");
    write_layer(&meta, &prompt_content, &dest)?;

    Ok(dest)
}

// ── layers export ─────────────────────────────────────────────────────────────

fn export_layers(name: Option<&str>, output: &Path) -> anyhow::Result<()> {
    let layers_dir = PathBuf::from("layers");

    if !layers_dir.exists() {
        anyhow::bail!("layers/ directory not found in current directory");
    }

    let layer_dirs = collect_layer_dirs(&layers_dir, name)?;

    if layer_dirs.is_empty() {
        if let Some(n) = name {
            anyhow::bail!("Layer '{}' not found in layers/", n);
        } else {
            println!("No layers found in layers/");
            return Ok(());
        }
    }

    let mut count = 0usize;
    for (layer_ref, dir) in &layer_dirs {
        match fix_and_export_layer(layer_ref, dir, output) {
            Ok(dest) => {
                println!("{} Exported {} → {}/", "✓".green(), layer_ref, dest.display());
                count += 1;
            }
            Err(e) => {
                eprintln!("{} Skipping '{}': {}", "⚠".yellow(), layer_ref, e);
            }
        }
    }

    println!("  {} layer(s) exported to {}/", count, output.display());
    Ok(())
}

/// Walk layers/ and collect (layer_ref_string, dir) pairs.
/// Handles both versioned (ns/name/version/) and unversioned (ns/name/) layouts.
fn collect_layer_dirs(
    layers_dir: &Path,
    name_filter: Option<&str>,
) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut results = Vec::new();

    // Walk up to 3 levels: layers/{ns}/{name}/{version}/ or layers/{ns}/{name}/
    let namespaces = std::fs::read_dir(layers_dir)?.flatten();
    for ns_entry in namespaces {
        let ns_path = ns_entry.path();
        if !ns_path.is_dir() {
            continue;
        }
        let ns_name = match ns_path.file_name().and_then(|n| n.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        let names = std::fs::read_dir(&ns_path)?.flatten();
        for name_entry in names {
            let name_path = name_entry.path();
            if !name_path.is_dir() {
                continue;
            }
            let layer_name = match name_path.file_name().and_then(|n| n.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };

            let full_ref = format!("{}/{}", ns_name, layer_name);

            // Apply name filter
            if let Some(filter) = name_filter {
                if full_ref != filter && layer_name != filter {
                    continue;
                }
            }

            // Check if this directory contains layer.yaml directly (unversioned)
            if name_path.join("layer.yaml").exists() {
                results.push((full_ref, name_path));
                continue;
            }

            // Otherwise look for versioned subdirectories
            let versions = std::fs::read_dir(&name_path)?.flatten();
            for ver_entry in versions {
                let ver_path = ver_entry.path();
                if !ver_path.is_dir() {
                    continue;
                }
                if ver_path.join("layer.yaml").exists() {
                    let ver_name = ver_path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("v1.0");
                    let versioned_ref = format!("{}/{}:{}", ns_name, layer_name, ver_name);
                    results.push((versioned_ref, ver_path));
                }
            }
        }
    }

    results.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(results)
}

fn fix_and_export_layer(layer_ref: &str, dir: &Path, output: &Path) -> anyhow::Result<PathBuf> {
    let yaml_path = dir.join("layer.yaml");
    let prompt_path = dir.join("prompt.md");

    let yaml_content = std::fs::read_to_string(&yaml_path)
        .with_context(|| format!("Cannot read {}", yaml_path.display()))?;

    let mut meta: LayerMeta = serde_yaml::from_str(&yaml_content)
        .with_context(|| format!("Cannot parse {}", yaml_path.display()))?;

    let prompt_content = if prompt_path.exists() {
        std::fs::read_to_string(&prompt_path)?
    } else {
        String::new()
    };

    // Infer namespace from layer_ref or directory structure if missing
    if meta.namespace.is_empty() {
        // Parse ns from "ns/name" or "ns/name:version"
        let ref_without_version = layer_ref.split(':').next().unwrap_or(layer_ref);
        if let Some(slash_pos) = ref_without_version.find('/') {
            meta.namespace = ref_without_version[..slash_pos].to_string();
        }
    }

    // Infer version from layer_ref if missing
    if meta.version.is_empty() {
        if let Some(colon_pos) = layer_ref.rfind(':') {
            meta.version = layer_ref[colon_pos + 1..].to_string();
        } else {
            meta.version = "v1.0".to_string();
        }
    }

    // Destination: {output}/{ns}/{name}/{version}/
    let dest = output
        .join(&meta.namespace)
        .join(&meta.name)
        .join(&meta.version);

    write_layer(&meta, &prompt_content, &dest)?;
    Ok(dest)
}

// ── SKILL.md parser ───────────────────────────────────────────────────────────

pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    pub argument_hint: Option<String>,
}

/// Parse SKILL.md into (frontmatter, body).
/// Frontmatter is YAML between the first two `---` lines.
fn parse_skill_md(content: &str) -> anyhow::Result<(SkillFrontmatter, String)> {
    let lines: Vec<&str> = content.lines().collect();

    // Find frontmatter delimiters
    if lines.first().map(|l| l.trim()) == Some("---") {
        // Find the closing ---
        let end = lines[1..].iter().position(|l| l.trim() == "---");
        if let Some(end_idx) = end {
            let fm_lines = &lines[1..end_idx + 1];
            let fm_yaml = fm_lines.join("\n");
            let fm = parse_frontmatter(&fm_yaml)?;
            let body_start = end_idx + 2; // skip opening --- and closing ---
            let body = if body_start < lines.len() {
                lines[body_start..].join("\n")
            } else {
                String::new()
            };
            return Ok((fm, body));
        }
    }

    // No frontmatter — treat entire content as body with empty metadata
    Ok((
        SkillFrontmatter {
            name: String::new(),
            description: String::new(),
            argument_hint: None,
        },
        content.to_string(),
    ))
}

fn parse_frontmatter(yaml: &str) -> anyhow::Result<SkillFrontmatter> {
    let value: serde_yaml::Value = serde_yaml::from_str(yaml)
        .with_context(|| "Failed to parse SKILL.md frontmatter")?;

    let name = value.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let description = value.get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let argument_hint = value.get("argument-hint")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(SkillFrontmatter { name, description, argument_hint })
}

// ── file writer ───────────────────────────────────────────────────────────────

fn write_layer(meta: &LayerMeta, prompt: &str, dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Cannot create directory: {}", dir.display()))?;

    let yaml = serde_yaml::to_string(meta)
        .with_context(|| "Failed to serialize layer.yaml")?;

    std::fs::write(dir.join("layer.yaml"), yaml)
        .with_context(|| format!("Cannot write {}/layer.yaml", dir.display()))?;

    std::fs::write(dir.join("prompt.md"), prompt)
        .with_context(|| format!("Cannot write {}/prompt.md", dir.display()))?;

    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn skills_base_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("skills")
}

fn git_user_name() -> Option<String> {
    std::process::Command::new("git")
        .args(["config", "--get", "user.name"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// ── similarity analysis ───────────────────────────────────────────────────────

fn collapse_blank_lines(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_blank = false;
    for line in s.lines() {
        let is_blank = line.trim().is_empty();
        if is_blank && prev_blank {
            continue;
        }
        result.push_str(line);
        result.push('\n');
        prev_blank = is_blank;
    }
    result
}

fn strip_frontmatter(content: &str) -> String {
    if content.trim_start().starts_with("---") {
        let after_first = content.trim_start().trim_start_matches("---");
        if let Some(end) = after_first.find("\n---") {
            return after_first[end + 4..].trim_start().to_string();
        }
    }
    content.to_string()
}

fn load_skill_contents() -> anyhow::Result<Vec<prompthub::similarity::SkillContent>> {
    use prompthub::similarity::{split_into_chunks, SkillContent};

    let skills_dir = skills_base_dir();
    let mut skill_contents: Vec<SkillContent> = Vec::new();

    if !skills_dir.exists() {
        return Ok(skill_contents);
    }

    for entry in std::fs::read_dir(&skills_dir)
        .with_context(|| format!("Cannot read skills directory: {}", skills_dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let skill_name = entry.file_name().to_string_lossy().to_string();
        let skill_md_path = entry.path().join("SKILL.md");

        if skill_md_path.exists() {
            let content = std::fs::read_to_string(&skill_md_path)
                .with_context(|| format!("Cannot read {}", skill_md_path.display()))?;
            let body = strip_frontmatter(&content);
            let chunks = split_into_chunks(&body);
            if !chunks.is_empty() {
                skill_contents.push(SkillContent { name: skill_name, chunks });
            }
        }
    }

    Ok(skill_contents)
}

fn run_similarity_analysis(_layers_dir: &Path) -> anyhow::Result<()> {
    use prompthub::similarity::{find_common_chunks, generate_split_plan};

    let skill_contents = load_skill_contents()?;

    if skill_contents.len() < 2 {
        return Ok(());
    }

    let suggestions = find_common_chunks(&skill_contents, 0.85);
    if suggestions.is_empty() {
        println!("\n{} 未发现可复用的公共段落", "✓".green());
        return Ok(());
    }

    let plans = generate_split_plan(&suggestions);

    println!(
        "\n{} 发现 {} 处可复用的公共段落：\n",
        "⚡".yellow(),
        suggestions.len()
    );

    for (i, plan) in plans.iter().enumerate() {
        println!(
            "  {}. 建议提取 {} (覆盖 {} 个 skill: {})",
            i + 1,
            plan.suggested_core_name.cyan(),
            plan.affected_skills.len(),
            plan.affected_skills.join(", ").dimmed(),
        );
        for chunk in &plan.common_chunks {
            println!(
                "     - 段落 \"{}\" (相似度 {:.0}%)",
                chunk.heading,
                chunk.avg_similarity * 100.0
            );
        }
    }

    println!(
        "\n  运行 {} 自动生成层结构",
        "ph export skills --refactor".cyan()
    );

    Ok(())
}

fn run_refactor(layers_dir: &Path, skip_confirm: bool) -> anyhow::Result<()> {
    use prompthub::similarity::{extract_core_content, find_common_chunks, generate_split_plan};

    let skill_contents = load_skill_contents()?;
    if skill_contents.is_empty() {
        anyhow::bail!("No skills found in {}", skills_base_dir().display());
    }

    if skill_contents.len() < 2 {
        println!("Not enough skills to analyze (need >= 2).");
        return Ok(());
    }

    let suggestions = find_common_chunks(&skill_contents, 0.85);
    if suggestions.is_empty() {
        println!("{} No common chunks found -- nothing to refactor.", "✓".green());
        return Ok(());
    }

    let plans = generate_split_plan(&suggestions);

    // Print preview of what will be created
    println!("\n{} Refactor plan:\n", "⚡".yellow());
    for (i, plan) in plans.iter().enumerate() {
        println!(
            "  {}. Create {} from {} skills ({})",
            i + 1,
            plan.suggested_core_name.cyan(),
            plan.affected_skills.len(),
            plan.affected_skills.join(", ").dimmed(),
        );
        for chunk in &plan.common_chunks {
            println!("     + extract \"{}\"", chunk.heading);
        }
    }

    // Confirm if needed
    if !skip_confirm {
        print!("\nProceed? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Execute each plan
    for plan in &plans {
        // Build core layer content
        let chunk_refs: Vec<&prompthub::similarity::CommonChunkSuggestion> =
            plan.common_chunks.iter().collect();
        let core_content = extract_core_content(&chunk_refs);

        // Determine core layer sections
        let core_sections: Vec<String> = plan
            .common_chunks
            .iter()
            .map(|c| prompthub::similarity::heading_to_section_name(&c.heading))
            .collect();

        // Write core layer
        let core_parts: Vec<&str> = plan.suggested_core_name.splitn(2, '/').collect();
        let (core_ns, core_name) = if core_parts.len() == 2 {
            (core_parts[0], core_parts[1])
        } else {
            ("common", plan.suggested_core_name.as_str())
        };

        let core_meta = prompthub::layer::LayerMeta {
            name: core_name.to_string(),
            namespace: core_ns.to_string(),
            version: "v1.0".to_string(),
            description: format!(
                "Shared core layer for: {}",
                plan.affected_skills.join(", ")
            ),
            author: git_user_name().unwrap_or_default(),
            tags: Vec::new(),
            sections: core_sections,
            conflicts: Vec::new(),
            requires: Vec::new(),
            models: Vec::new(),
            language: None,
            family: None,
        };
        let core_dir = layers_dir.join(core_ns).join(core_name).join("v1.0");
        write_layer(&core_meta, &core_content, &core_dir)?;
        println!("{} Created {}/", "✓".green(), core_dir.display());

        // For each affected skill: add requires, remove common chunk sections from prompt.md
        let requires_ref = format!("{}/{}:v1.0", core_ns, core_name);
        let common_headings: std::collections::HashSet<&str> =
            plan.common_chunks.iter().map(|c| c.heading.as_str()).collect();

        for skill_name in &plan.affected_skills {
            let skill_layer_dir = layers_dir.join("skill").join(skill_name).join("v1.0");
            if !skill_layer_dir.exists() {
                continue;
            }

            // Update layer.yaml: add requires
            let yaml_path = skill_layer_dir.join("layer.yaml");
            if yaml_path.exists() {
                // Backup
                std::fs::copy(&yaml_path, yaml_path.with_extension("yaml.bak"))?;

                let yaml_str = std::fs::read_to_string(&yaml_path)?;
                let mut meta: prompthub::layer::LayerMeta = serde_yaml::from_str(&yaml_str)
                    .with_context(|| format!("Cannot parse {}", yaml_path.display()))?;
                if !meta.requires.contains(&requires_ref) {
                    meta.requires.push(requires_ref.clone());
                }
                let new_yaml = serde_yaml::to_string(&meta)?;
                std::fs::write(&yaml_path, new_yaml)?;
            }

            // Update prompt.md: remove common chunk body text.
            // The exported prompt.md uses [section] format, not ## headings, so we
            // cannot use split_into_chunks here. Instead, remove the representative
            // body of each common chunk by direct text substitution.
            let prompt_path = skill_layer_dir.join("prompt.md");
            if prompt_path.exists() {
                // Backup
                std::fs::copy(&prompt_path, prompt_path.with_extension("md.bak"))?;

                let mut prompt_str = std::fs::read_to_string(&prompt_path)?;
                for chunk in &plan.common_chunks {
                    let body_trimmed = chunk.representative_body.trim();
                    if !body_trimmed.is_empty() {
                        prompt_str = prompt_str.replace(body_trimmed, "");
                    }
                }
                // Collapse runs of blank lines left by removal
                let new_prompt = collapse_blank_lines(&prompt_str);
                std::fs::write(&prompt_path, new_prompt)?;
            }

            println!(
                "{} Updated skill/{}/v1.0 (requires: {})",
                "✓".green(),
                skill_name,
                requires_ref
            );
        }
    }

    println!("\n{} Refactor complete.", "✓".green());
    Ok(())
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skill_md_with_frontmatter() {
        let content = r#"---
name: humanizer
description: Rewrites text to sound more natural
argument-hint: "<text to humanize>"
---
[instructions]
Rewrite the following text.
"#;
        let (fm, body) = parse_skill_md(content).unwrap();
        assert_eq!(fm.name, "humanizer");
        assert_eq!(fm.description, "Rewrites text to sound more natural");
        assert_eq!(fm.argument_hint.as_deref(), Some("<text to humanize>"));
        assert!(body.contains("[instructions]"));
    }

    #[test]
    fn test_parse_skill_md_no_frontmatter() {
        let content = "Just some plain instructions without frontmatter.\n";
        let (fm, body) = parse_skill_md(content).unwrap();
        assert!(fm.name.is_empty());
        assert!(fm.description.is_empty());
        assert!(fm.argument_hint.is_none());
        assert!(body.contains("plain instructions"));
    }

    #[test]
    fn test_parse_skill_md_frontmatter_no_body() {
        let content = "---\nname: empty-skill\ndescription: No body\n---\n";
        let (fm, body) = parse_skill_md(content).unwrap();
        assert_eq!(fm.name, "empty-skill");
        assert!(body.trim().is_empty());
    }

    #[test]
    fn test_export_skills_wraps_plain_body_in_instructions() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("skills").join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();

        let skill_md = "---\nname: my-skill\ndescription: A test skill\n---\nDo this thing.\n";
        std::fs::write(skill_dir.join("SKILL.md"), skill_md).unwrap();

        let output = tmp.path().join("output");
        export_one_skill("my-skill", &skill_dir, &output).unwrap();

        let prompt = std::fs::read_to_string(
            output.join("skill").join("my-skill").join("v1.0").join("prompt.md")
        ).unwrap();
        assert!(prompt.contains("[instructions]"), "plain body should be wrapped in [instructions]");
    }

    #[test]
    fn test_export_skills_preserves_sections() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("skills").join("sectioned-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();

        let skill_md = "---\nname: sectioned-skill\ndescription: Has sections\n---\n[role]\nBe expert.\n\n[constraints]\nNo hallucinations.\n";
        std::fs::write(skill_dir.join("SKILL.md"), skill_md).unwrap();

        let output = tmp.path().join("output");
        export_one_skill("sectioned-skill", &skill_dir, &output).unwrap();

        let yaml_content = std::fs::read_to_string(
            output.join("skill").join("sectioned-skill").join("v1.0").join("layer.yaml")
        ).unwrap();
        let meta: LayerMeta = serde_yaml::from_str(&yaml_content).unwrap();
        assert!(meta.sections.contains(&"role".to_string()));
        assert!(meta.sections.contains(&"constraints".to_string()));
    }

    #[test]
    fn test_write_layer_creates_files() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let meta = LayerMeta {
            name: "test".to_string(),
            namespace: "skill".to_string(),
            version: "v1.0".to_string(),
            description: "Test layer".to_string(),
            author: String::new(),
            tags: Vec::new(),
            sections: vec!["instructions".to_string()],
            conflicts: Vec::new(),
            requires: Vec::new(),
            models: Vec::new(),
            language: None,
            family: None,
        };

        write_layer(&meta, "[instructions]\nTest content.\n", tmp.path()).unwrap();

        assert!(tmp.path().join("layer.yaml").exists());
        assert!(tmp.path().join("prompt.md").exists());

        let loaded = prompthub::layer::Layer::load_from_dir(tmp.path()).unwrap();
        assert_eq!(loaded.meta.name, "test");
        assert_eq!(loaded.meta.namespace, "skill");
    }
}
