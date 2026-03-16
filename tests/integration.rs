use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// Re-export internal modules for testing
// We compile the library as a binary-only crate, so we test by calling the public API directly.

fn create_layer(dir: &Path, namespace: &str, name: &str, version: &str,
                sections: &[(&str, &str)], conflicts: &[&str]) {
    let layer_dir = dir.join(namespace).join(name).join(version);
    fs::create_dir_all(&layer_dir).unwrap();

    let sections_list: Vec<String> = sections.iter().map(|(n, _)| n.to_string()).collect();
    let conflicts_list: Vec<String> = conflicts.iter().map(|s| format!("\"{}\"", s)).collect();

    let yaml = format!(
        "name: {}\nnamespace: {}\nversion: {}\ndescription: \"Test {}\"\nauthor: test\ntags: [test]\nsections: [{}]\nconflicts: [{}]\nrequires: []\nmodels: []\n",
        name, namespace, version, name,
        sections_list.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(", "),
        conflicts_list.join(", ")
    );
    fs::write(layer_dir.join("layer.yaml"), yaml).unwrap();

    let mut prompt = String::new();
    for (section_name, content) in sections {
        prompt.push_str(&format!("[{}]\n{}\n\n", section_name, content));
    }
    fs::write(layer_dir.join("prompt.md"), prompt).unwrap();
}

fn create_promptfile(dir: &Path, content: &str) {
    fs::write(dir.join("Promptfile"), content).unwrap();
}

// ── Test 1: Single FROM layer build ──────────────────────────────────────────

#[test]
fn test_single_from_build() {
    let tmp = TempDir::new().unwrap();
    let layers_dir = tmp.path().join("layers");

    create_layer(&layers_dir, "base", "reviewer", "v1.0", &[
        ("role", "You are a code reviewer."),
        ("constraints", "Be concise."),
    ], &[]);

    create_promptfile(tmp.path(), "FROM base/reviewer:v1.0\n");

    // Parse the Promptfile
    let pf_content = fs::read_to_string(tmp.path().join("Promptfile")).unwrap();
    let pf = prompthub::parser::parse(&pf_content).unwrap();

    assert_eq!(pf.from.source, "base/reviewer");
    assert_eq!(pf.from.version, "v1.0");
    assert!(pf.layers.is_empty());

    // Resolve
    let resolver = prompthub::resolver::LayerResolver::new(vec![layers_dir]);
    let base = resolver.resolve(&pf.from).unwrap();
    assert!(base.sections.contains_key("role"));
    assert!(base.sections.contains_key("constraints"));

    // Merge (single layer)
    let merged = prompthub::merger::merge_layers(&base, &[], HashMap::new()).unwrap();
    let text = merged.to_text();
    assert!(text.contains("You are a code reviewer."));
    assert!(text.contains("Be concise."));
}

// ── Test 2: Multi-layer with section override ─────────────────────────────────

#[test]
fn test_multi_layer_section_override() {
    let tmp = TempDir::new().unwrap();
    let layers_dir = tmp.path().join("layers");

    create_layer(&layers_dir, "base", "writer", "v1.0", &[
        ("role", "You are a writer."),
        ("constraints", "Write clearly."),
    ], &[]);

    create_layer(&layers_dir, "style", "concise", "v1.0", &[
        ("constraints", "Be very concise. Under 100 words."),
    ], &[]);

    create_promptfile(tmp.path(),
        "FROM base/writer:v1.0\nLAYER style/concise:v1.0\n"
    );

    let pf_content = fs::read_to_string(tmp.path().join("Promptfile")).unwrap();
    let pf = prompthub::parser::parse(&pf_content).unwrap();

    let resolver = prompthub::resolver::LayerResolver::new(vec![layers_dir]);
    let base = resolver.resolve(&pf.from).unwrap();
    let extra = resolver.resolve(&pf.layers[0]).unwrap();

    let merged = prompthub::merger::merge_layers(&base, &[extra], HashMap::new()).unwrap();

    // style/concise overrides the "constraints" section
    assert_eq!(merged.sections["constraints"], "Be very concise. Under 100 words.");
    // role is kept from base
    assert!(merged.sections["role"].contains("You are a writer."));
    // Warning was generated
    assert_eq!(merged.warnings.len(), 1);
    assert!(merged.warnings[0].contains("constraints"));
}

// ── Test 3: VAR variable substitution ─────────────────────────────────────────

#[test]
fn test_var_substitution() {
    let tmp = TempDir::new().unwrap();
    let layers_dir = tmp.path().join("layers");

    create_layer(&layers_dir, "base", "translator", "v1.0", &[
        ("role", "Translate to ${target_lang}."),
    ], &[]);

    create_promptfile(tmp.path(),
        "FROM base/translator:v1.0\nVAR target_lang \"Spanish\"\nTASK \"Translate: hello\"\n"
    );

    let pf_content = fs::read_to_string(tmp.path().join("Promptfile")).unwrap();
    let pf = prompthub::parser::parse(&pf_content).unwrap();

    assert_eq!(pf.vars["target_lang"], "Spanish");
    assert_eq!(pf.task.as_deref(), Some("Translate: hello"));

    let resolver = prompthub::resolver::LayerResolver::new(vec![layers_dir]);
    let base = resolver.resolve(&pf.from).unwrap();
    let merged = prompthub::merger::merge_layers(&base, &[], pf.params.clone()).unwrap();

    let text = prompthub::renderer::render_variables(
        &merged,
        &pf.vars,
        pf.task.as_deref(),
        &[],
        tmp.path(),
    ).unwrap();

    assert!(text.contains("Translate to Spanish."), "Got: {}", text);
    assert!(text.contains("Translate: hello"), "Got: {}", text);
}

// ── Test 4: INCLUDE file ───────────────────────────────────────────────────────

#[test]
fn test_include_file() {
    let tmp = TempDir::new().unwrap();
    let layers_dir = tmp.path().join("layers");

    create_layer(&layers_dir, "base", "analyst", "v1.0", &[
        ("role", "You are a data analyst."),
    ], &[]);

    // Create an include file
    fs::write(tmp.path().join("context.md"), "## Data Context\nThe dataset has 1000 rows.").unwrap();

    create_promptfile(tmp.path(),
        "FROM base/analyst:v1.0\nINCLUDE ./context.md\n"
    );

    let pf_content = fs::read_to_string(tmp.path().join("Promptfile")).unwrap();
    let pf = prompthub::parser::parse(&pf_content).unwrap();

    assert_eq!(pf.includes.len(), 1);

    // Load include
    let include_content = prompthub::renderer::load_include(&pf.includes[0], tmp.path()).unwrap();
    assert!(include_content.contains("1000 rows"));

    let includes = vec![(pf.includes[0].clone(), include_content)];

    let resolver = prompthub::resolver::LayerResolver::new(vec![layers_dir]);
    let base = resolver.resolve(&pf.from).unwrap();
    let merged = prompthub::merger::merge_layers(&base, &[], HashMap::new()).unwrap();

    let text = prompthub::renderer::render_variables(
        &merged,
        &pf.vars,
        None,
        &includes,
        tmp.path(),
    ).unwrap();

    assert!(text.contains("data analyst"), "Got: {}", text);
    assert!(text.contains("1000 rows"), "Got: {}", text);
}

// ── Test 5: Conflict detection ─────────────────────────────────────────────────

#[test]
fn test_conflict_detection() {
    let tmp = TempDir::new().unwrap();
    let layers_dir = tmp.path().join("layers");

    // writer conflicts with translator
    create_layer(&layers_dir, "base", "writer", "v1.0", &[
        ("role", "You are a writer."),
    ], &["base/translator"]);

    create_layer(&layers_dir, "base", "translator", "v1.0", &[
        ("role", "You are a translator."),
    ], &[]);

    create_promptfile(tmp.path(),
        "FROM base/writer:v1.0\nLAYER base/translator:v1.0\n"
    );

    let pf_content = fs::read_to_string(tmp.path().join("Promptfile")).unwrap();
    let pf = prompthub::parser::parse(&pf_content).unwrap();

    let resolver = prompthub::resolver::LayerResolver::new(vec![layers_dir]);
    let base = resolver.resolve(&pf.from).unwrap();
    let extra = resolver.resolve(&pf.layers[0]).unwrap();

    let result = prompthub::merger::merge_layers(&base, &[extra], HashMap::new());
    assert!(result.is_err(), "Expected conflict error");
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(err_str.contains("conflict") || err_str.contains("Conflict"), "Got: {}", err_str);
}

// ── Test 6: Semver version sorting (v1.9 vs v1.10) ────────────────────────────

#[test]
fn test_resolve_semver_latest_ordering() {
    let tmp = TempDir::new().unwrap();
    let layers_dir = tmp.path().join("layers");

    // Create v1.9 and v1.10 — lexicographic order would pick v1.9 as "latest"
    create_layer(&layers_dir, "base", "semver-test", "v1.9", &[
        ("role", "Version 1.9"),
    ], &[]);
    create_layer(&layers_dir, "base", "semver-test", "v1.10", &[
        ("role", "Version 1.10"),
    ], &[]);

    let resolver = prompthub::resolver::LayerResolver::new(vec![layers_dir]);
    let layer_ref = prompthub::parser::LayerRef {
        source: "base/semver-test".to_string(),
        version: "latest".to_string(),
    };
    let layer = resolver.resolve(&layer_ref).unwrap();
    assert_eq!(layer.meta.version, "v1.10",
        "Semver sort must pick v1.10 as latest, not v1.9");
}
