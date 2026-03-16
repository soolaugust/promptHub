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

    let (text, undef) = prompthub::renderer::render_variables(
        &merged,
        &pf.vars,
        pf.task.as_deref(),
        &[],
    ).unwrap();

    assert!(undef.is_empty(), "unexpected undefined vars: {:?}", undef);
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

    let (text, _undef) = prompthub::renderer::render_variables(
        &merged,
        &pf.vars,
        None,
        &includes,
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

// ── Test 7: Section-override warning is generated ──────────────────────────────


#[test]
fn test_section_override_warning_generated() {
    let tmp = TempDir::new().unwrap();
    let layers_dir = tmp.path().join("layers");

    create_layer(&layers_dir, "base", "writer", "v1.0", &[
        ("role", "You are a writer."),
        ("constraints", "Original constraint."),
    ], &[]);
    create_layer(&layers_dir, "style", "override-style", "v1.0", &[
        ("constraints", "Overriding constraint."),
    ], &[]);

    let pf_content = "FROM base/writer:v1.0\nLAYER style/override-style:v1.0\n";
    let pf = prompthub::parser::parse(pf_content).unwrap();

    let resolver = prompthub::resolver::LayerResolver::new(vec![layers_dir]);
    let base = resolver.resolve(&pf.from).unwrap();
    let extra = resolver.resolve(&pf.layers[0]).unwrap();
    let merged = prompthub::merger::merge_layers(&base, &[extra], HashMap::new()).unwrap();

    assert!(!merged.warnings.is_empty(),
        "Overriding a section should generate a warning");
    assert!(merged.warnings[0].contains("constraints"),
        "Warning should mention the overridden section name");
}

// ── Test 8: Undefined variable warning is returned to caller ──────────────────

#[test]
fn test_undefined_var_warning_returned() {
    let tmp = TempDir::new().unwrap();
    let layers_dir = tmp.path().join("layers");

    // Layer uses ${missing_var} which is not defined in the Promptfile
    create_layer(&layers_dir, "base", "template", "v1.0", &[
        ("role", "Answer in ${missing_var}."),
    ], &[]);

    let pf_content = "FROM base/template:v1.0\n";
    let pf = prompthub::parser::parse(pf_content).unwrap();

    let resolver = prompthub::resolver::LayerResolver::new(vec![layers_dir]);
    let base = resolver.resolve(&pf.from).unwrap();
    let merged = prompthub::merger::merge_layers(&base, &[], pf.params.clone()).unwrap();

    let (text, undef) = prompthub::renderer::render_variables(
        &merged,
        &pf.vars,  // empty vars map
        None,
        &[],
    ).unwrap();

    // The placeholder should be kept verbatim
    assert!(text.contains("${missing_var}"), "undefined var should stay as-is in output");
    // And the variable name should be reported
    assert_eq!(undef, vec!["missing_var".to_string()],
        "undefined variable name should be reported in the warnings list");
}

// ── Test 9: Three-layer merge (base + 2 additional) ───────────────────────────

#[test]
fn test_three_layer_merge() {
    let tmp = TempDir::new().unwrap();
    let layers_dir = tmp.path().join("layers");

    // Base layer has role and constraints
    create_layer(&layers_dir, "base", "writer", "v1.0", &[
        ("role", "You are a professional writer."),
        ("constraints", "Write clearly."),
        ("output-format", "Use paragraphs."),
    ], &[]);

    // Style layer overrides constraints
    create_layer(&layers_dir, "style", "concise", "v1.0", &[
        ("constraints", "Be very concise. Under 50 words."),
    ], &[]);

    // Language layer adds a new section
    create_layer(&layers_dir, "lang", "spanish", "v1.0", &[
        ("language", "Respond in Spanish."),
    ], &[]);

    let pf_content = "FROM base/writer:v1.0\nLAYER style/concise:v1.0\nLAYER lang/spanish:v1.0\n";
    let pf = prompthub::parser::parse(pf_content).unwrap();
    assert_eq!(pf.layers.len(), 2, "should have 2 additional layers");

    let resolver = prompthub::resolver::LayerResolver::new(vec![layers_dir]);
    let base = resolver.resolve(&pf.from).unwrap();
    let layer2 = resolver.resolve(&pf.layers[0]).unwrap();
    let layer3 = resolver.resolve(&pf.layers[1]).unwrap();

    let merged = prompthub::merger::merge_layers(&base, &[layer2, layer3], HashMap::new()).unwrap();

    // Constraints overridden by style/concise
    assert_eq!(merged.sections["constraints"], "Be very concise. Under 50 words.",
        "constraints should be overridden by second layer");

    // Role preserved from base
    assert!(merged.sections["role"].contains("professional writer"),
        "role section should be preserved from base layer");

    // Language added by third layer
    assert_eq!(merged.sections["language"], "Respond in Spanish.",
        "language section should be added by third layer");

    // Output-format preserved from base
    assert!(merged.sections["output-format"].contains("paragraphs"),
        "output-format section should be preserved from base layer");

    // Exactly one override warning (for constraints)
    assert_eq!(merged.warnings.len(), 1,
        "should have exactly one override warning");
    assert!(merged.warnings[0].contains("constraints"),
        "warning should mention the overridden section");

    // Full text should contain all non-overridden content
    let text = merged.to_text();
    assert!(text.contains("professional writer"), "role in text");
    assert!(text.contains("Under 50 words"), "overridden constraints in text");
    assert!(text.contains("Spanish"), "language section in text");
}

// ── Test 10: Additional layer conflict declaration ─────────────────────────────

#[test]
fn test_additional_layer_declares_conflict() {
    let tmp = TempDir::new().unwrap();
    let layers_dir = tmp.path().join("layers");

    // The *additional* layer declares a conflict with the base layer.
    create_layer(&layers_dir, "base", "writer", "v1.0", &[
        ("role", "You are a writer."),
    ], &[]);

    // translator declares it conflicts with writer
    create_layer(&layers_dir, "base", "translator", "v1.0", &[
        ("role", "You are a translator."),
    ], &["base/writer"]);

    let resolver = prompthub::resolver::LayerResolver::new(vec![layers_dir]);
    let base_ref = prompthub::parser::LayerRef {
        source: "base/writer".to_string(),
        version: "v1.0".to_string(),
    };
    let extra_ref = prompthub::parser::LayerRef {
        source: "base/translator".to_string(),
        version: "v1.0".to_string(),
    };

    let base = resolver.resolve(&base_ref).unwrap();
    let extra = resolver.resolve(&extra_ref).unwrap();

    let result = prompthub::merger::merge_layers(&base, &[extra], HashMap::new());
    assert!(result.is_err(),
        "conflict declared by additional layer should also be detected");
}

// ── Test 11: PARAM directives are threaded into merged result ─────────────────

#[test]
fn test_param_directives_in_promptfile() {
    let tmp = TempDir::new().unwrap();
    let layers_dir = tmp.path().join("layers");

    create_layer(&layers_dir, "base", "assistant", "v1.0", &[
        ("role", "You are a helpful assistant."),
    ], &[]);

    create_promptfile(tmp.path(),
        "FROM base/assistant:v1.0\nPARAM model \"claude-sonnet-4-6\"\nPARAM temperature \"0.3\"\n"
    );

    let pf_content = fs::read_to_string(tmp.path().join("Promptfile")).unwrap();
    let pf = prompthub::parser::parse(&pf_content).unwrap();

    // Params should be parsed from Promptfile
    assert_eq!(pf.params.get("model").map(String::as_str), Some("claude-sonnet-4-6"),
        "PARAM model should be parsed from Promptfile");
    assert_eq!(pf.params.get("temperature").map(String::as_str), Some("0.3"),
        "PARAM temperature should be parsed from Promptfile");

    // Params should be preserved through merge
    let resolver = prompthub::resolver::LayerResolver::new(vec![layers_dir]);
    let base = resolver.resolve(&pf.from).unwrap();
    let merged = prompthub::merger::merge_layers(&base, &[], pf.params.clone()).unwrap();

    assert_eq!(merged.params.get("model").map(String::as_str), Some("claude-sonnet-4-6"),
        "model param should survive merge");
    assert_eq!(merged.params.get("temperature").map(String::as_str), Some("0.3"),
        "temperature param should survive merge");
}

// ── Test 12: FROM with path-traversal is rejected ─────────────────────────────

#[test]
fn test_from_path_traversal_rejected() {
    // A Promptfile referencing a layer via path traversal should be rejected
    // by the parser before any filesystem access occurs.
    let pf_content = "FROM ../evil/layer:v1.0\n";
    let result = prompthub::parser::parse(pf_content);
    assert!(result.is_err(),
        "FROM with path-traversal component should be a parse error");
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("invalid") || err_msg.contains("..") || err_msg.contains("path"),
        "error message should indicate path traversal issue, got: {}", err_msg);
}

// ── Test 13: VAR override via parse_var_override matches Promptfile VAR ───────

#[test]
fn test_var_override_overrides_promptfile_var() {
    let tmp = TempDir::new().unwrap();
    let layers_dir = tmp.path().join("layers");

    create_layer(&layers_dir, "base", "translator", "v1.0", &[
        ("role", "Translate to ${target_lang}."),
    ], &[]);

    // Promptfile sets target_lang = "French"
    let pf_content = "FROM base/translator:v1.0\nVAR target_lang \"French\"\n";
    let mut pf = prompthub::parser::parse(pf_content).unwrap();

    // CLI-style override changes target_lang to "German"
    let (name, value) = prompthub::parser::parse_var_override("target_lang=German").unwrap();
    pf.vars.insert(name, value);

    let resolver = prompthub::resolver::LayerResolver::new(vec![layers_dir]);
    let base = resolver.resolve(&pf.from).unwrap();
    let merged = prompthub::merger::merge_layers(&base, &[], HashMap::new()).unwrap();

    let (text, undef) = prompthub::renderer::render_variables(
        &merged,
        &pf.vars,
        None,
        &[],
    ).unwrap();

    assert!(undef.is_empty(), "no undefined vars expected");
    assert!(text.contains("Translate to German."),
        "CLI override should replace Promptfile VAR value; got: {}", text);
    assert!(!text.contains("French"),
        "Promptfile default should be replaced by CLI override; got: {}", text);
}
