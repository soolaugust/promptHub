use crate::error::{PromptHubError, Result};
use crate::layer::Layer;
use crate::parser::LayerRef;
use crate::config::{global_layers_dir};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Parse a version string as semver, stripping a leading 'v' if present.
/// Handles two-part versions like "v1.9" by treating them as "1.9.0".
/// Exposed publicly so binaries (e.g. `ph`) can reuse it when sorting
/// version lists, avoiding duplication of the two-step parse logic.
pub fn parse_semver(dir_name: &str) -> Option<semver::Version> {
    let s = dir_name.strip_prefix('v').unwrap_or(dir_name);
    // Try exact parse first (e.g. "1.9.0")
    if let Ok(v) = semver::Version::parse(s) {
        return Some(v);
    }
    // Try appending ".0" for two-part versions like "1.9" -> "1.9.0"
    let padded = format!("{}.0", s);
    semver::Version::parse(&padded).ok()
}

/// Compare two version directory paths using semver ordering; fall back to
/// lexicographic ordering when the names are not valid semver.
fn cmp_version_dirs(a: &Path, b: &Path) -> std::cmp::Ordering {
    let name_a = a.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let name_b = b.file_name().and_then(|n| n.to_str()).unwrap_or("");
    match (parse_semver(name_a), parse_semver(name_b)) {
        (Some(va), Some(vb)) => va.cmp(&vb),
        _ => name_a.cmp(name_b),
    }
}

/// Resolve a LayerRef to an actual Layer by searching local paths
pub struct LayerResolver {
    /// Extra search paths (e.g. project-local layers/)
    extra_paths: Vec<PathBuf>,
}

impl LayerResolver {
    pub fn new(extra_paths: Vec<PathBuf>) -> Self {
        LayerResolver { extra_paths }
    }

    /// Resolve a LayerRef to a Layer
    pub fn resolve(&self, layer_ref: &LayerRef) -> Result<Layer> {
        let search_paths = self.search_paths();

        for base_path in &search_paths {
            if let Some(layer) = self.find_layer(base_path, layer_ref)? {
                return Ok(layer);
            }
        }

        Err(PromptHubError::LayerNotFound(
            format!("Layer '{}' not found. Run `prompthub pull {}` to fetch it.",
                layer_ref.display(), layer_ref.display())
        ))
    }

    /// Resolve a layer AND all its transitive `requires` dependencies.
    ///
    /// Returns `(target_layer, deps_in_order)` where `deps_in_order` contains
    /// all required base layers in dependency-first order (deepest base first).
    /// Dependencies are deduplicated — a shared dep appears only once.
    ///
    /// Cycles are detected and return an error. Diamond dependencies are deduplicated silently.
    pub fn resolve_with_requires(&self, layer_ref: &LayerRef) -> Result<(Layer, Vec<Layer>)> {
        let mut visited = std::collections::HashSet::new();
        let mut in_progress = std::collections::HashSet::new();
        let layer = self.resolve(layer_ref)?;
        let deps = self.collect_requires(&layer, &mut visited, &mut in_progress)?;
        Ok((layer, deps))
    }

    /// Recursively collect all `requires` layers in dependency-first order.
    fn collect_requires(
        &self,
        layer: &Layer,
        visited: &mut std::collections::HashSet<String>,
        in_progress: &mut std::collections::HashSet<String>,
    ) -> Result<Vec<Layer>> {
        let mut result = Vec::new();
        for req_str in &layer.meta.requires {
            let (source, version) = if let Some(pos) = req_str.rfind(':') {
                (&req_str[..pos], &req_str[pos + 1..])
            } else {
                (req_str.as_str(), "latest")
            };
            let key = format!("{}:{}", source, version);
            if in_progress.contains(&key) {
                return Err(PromptHubError::ValidationError(
                    format!("Circular dependency detected: '{}' is already being resolved", key)
                ));
            }
            if visited.contains(&key) {
                continue; // diamond dedup: fully processed, safe to skip
            }
            in_progress.insert(key.clone());

            let req_ref = LayerRef {
                source: source.to_string(),
                version: version.to_string(),
            };
            let req_layer = self.resolve(&req_ref)?;
            // Depth-first: collect the dep's own requires first (deepest base comes first)
            let mut transitive = self.collect_requires(&req_layer, visited, in_progress)?;
            result.append(&mut transitive);

            in_progress.remove(&key); // backtrack: no longer on the call stack
            visited.insert(key);      // mark as fully processed
            result.push(req_layer);
        }
        Ok(result)
    }

    fn search_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.extra_paths.clone();
        paths.push(global_layers_dir());
        paths
    }

    fn find_layer(&self, base: &Path, layer_ref: &LayerRef) -> Result<Option<Layer>> {
        // Convert "base/code-reviewer" -> base/code-reviewer/
        let layer_path = base.join(&layer_ref.source);

        if !layer_path.exists() {
            return Ok(None);
        }

        // Find matching version
        let version_dir = self.resolve_version(&layer_path, &layer_ref.version)?;

        if let Some(dir) = version_dir {
            let layer = Layer::load_from_dir(&dir)?;
            return Ok(Some(layer));
        }

        Ok(None)
    }

    fn resolve_version(&self, layer_path: &Path, version: &str) -> Result<Option<PathBuf>> {
        // Check if the layer_path itself contains layer.yaml (flat structure)
        if layer_path.join("layer.yaml").exists() {
            return Ok(Some(layer_path.to_path_buf()));
        }

        // List version subdirectories
        let mut versions: Vec<PathBuf> = std::fs::read_dir(layer_path)
            .map_err(|e| PromptHubError::Other(format!("{}: {}", layer_path.display(), e)))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|entry| entry.path())
            .collect();

        if versions.is_empty() {
            return Ok(None);
        }

        // Sort versions using semver ordering so v1.10 > v1.9
        versions.sort_by(|a, b| cmp_version_dirs(a, b));

        match version {
            "latest" | "" => {
                // Return the latest (last when sorted)
                Ok(versions.last().cloned())
            }
            v if v.starts_with('v') && !v.contains('.') => {
                // Major version match: "v1" matches "v1.0", "v1.1", etc.
                // We require the directory name to equal the major prefix OR
                // start with "<major>." to avoid "v1" matching "v10", "v11", etc.
                let major = v;
                let major_dot = format!("{}.", major);
                let matching: Vec<&PathBuf> = versions.iter()
                    .filter(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| n == major || n.starts_with(major_dot.as_str()))
                            .unwrap_or(false)
                    })
                    .collect();
                Ok(matching.last().cloned().cloned())
            }
            v => {
                // Exact version match
                Ok(versions.iter().find(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n == v)
                        .unwrap_or(false)
                }).cloned())
            }
        }
    }
}

/// Scan a directory for all available layers (returns list of full names like "base/code-reviewer")
pub fn scan_layers(base_dir: &Path) -> Vec<(String, PathBuf)> {
    if !base_dir.exists() {
        return Vec::new();
    }

    let mut results = Vec::new();

    for entry in WalkDir::new(base_dir).min_depth(1).max_depth(4) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if entry.file_type().is_file() && entry.file_name() == "layer.yaml" {
            if let Some(layer_dir) = entry.path().parent() {
                // Get relative path from base_dir
                if let Ok(rel) = layer_dir.strip_prefix(base_dir) {
                    let name = rel.to_string_lossy().replace('\\', "/");
                    results.push((name, layer_dir.to_path_buf()));
                }
            }
        }
    }

    results.sort_by(|a, b| a.0.cmp(&b.0));
    results
}

/// Search for layers matching a keyword across multiple base directories.
///
/// Each directory is scanned via `scan_layers`; layers whose name, description,
/// or any tag contains `keyword` (case-insensitive) are returned as a list of
/// `(display_name, Layer)` pairs.  Errors loading individual layers are silently
/// skipped so that one corrupted layer doesn't abort the entire search.
pub fn search_layers(
    dirs: &[PathBuf],
    keyword: &str,
) -> Vec<(String, crate::layer::Layer)> {
    let kw = keyword.to_lowercase();
    let mut results = Vec::new();

    for base in dirs {
        for (name, path) in scan_layers(base) {
            if let Ok(l) = crate::layer::Layer::load_from_dir(&path) {
                let matches = name.to_lowercase().contains(&kw)
                    || l.meta.description.to_lowercase().contains(&kw)
                    || l.meta.tags.iter().any(|t| t.to_lowercase().contains(&kw));
                if matches {
                    results.push((name, l));
                }
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    fn create_test_layer(dir: &Path, name: &str, namespace: &str, version: &str) {
        let layer_dir = dir.join(namespace).join(name).join(version);
        fs::create_dir_all(&layer_dir).unwrap();

        let yaml = format!(r#"name: {}
namespace: {}
version: {}
description: "Test layer"
author: test
tags: []
sections: [role]
conflicts: []
requires: []
models: []
"#, name, namespace, version);
        fs::write(layer_dir.join("layer.yaml"), yaml).unwrap();
        fs::write(layer_dir.join("prompt.md"), "[role]\nTest content\n").unwrap();
    }

    #[test]
    fn test_resolve_exact_version() {
        let tmp = TempDir::new().unwrap();
        create_test_layer(tmp.path(), "code-reviewer", "base", "v1.0");

        let resolver = LayerResolver::new(vec![tmp.path().to_path_buf()]);
        let layer_ref = LayerRef { source: "base/code-reviewer".to_string(), version: "v1.0".to_string() };
        let layer = resolver.resolve(&layer_ref).unwrap();
        assert_eq!(layer.meta.name, "code-reviewer");
    }

    #[test]
    fn test_resolve_latest() {
        let tmp = TempDir::new().unwrap();
        create_test_layer(tmp.path(), "code-reviewer", "base", "v1.0");
        create_test_layer(tmp.path(), "code-reviewer", "base", "v1.1");

        let resolver = LayerResolver::new(vec![tmp.path().to_path_buf()]);
        let layer_ref = LayerRef { source: "base/code-reviewer".to_string(), version: "latest".to_string() };
        let layer = resolver.resolve(&layer_ref).unwrap();
        assert_eq!(layer.meta.version, "v1.1");
    }

    #[test]
    fn test_resolve_not_found() {
        let tmp = TempDir::new().unwrap();
        let resolver = LayerResolver::new(vec![tmp.path().to_path_buf()]);
        let layer_ref = LayerRef { source: "base/nonexistent".to_string(), version: "latest".to_string() };
        assert!(resolver.resolve(&layer_ref).is_err());
    }

    #[test]
    fn test_scan_layers() {
        let tmp = TempDir::new().unwrap();
        create_test_layer(tmp.path(), "code-reviewer", "base", "v1.0");
        create_test_layer(tmp.path(), "concise", "style", "v1.0");

        let layers = scan_layers(tmp.path());
        assert_eq!(layers.len(), 2);
        let names: Vec<&str> = layers.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"base/code-reviewer/v1.0"));
    }

    #[test]
    fn test_resolve_major_version() {
        let tmp = TempDir::new().unwrap();
        create_test_layer(tmp.path(), "code-reviewer", "base", "v1.0");
        create_test_layer(tmp.path(), "code-reviewer", "base", "v1.1");
        create_test_layer(tmp.path(), "code-reviewer", "base", "v2.0");

        let resolver = LayerResolver::new(vec![tmp.path().to_path_buf()]);
        // "v1" should match v1.0 and v1.1 but not v2.0, returning v1.1 as latest
        let layer_ref = LayerRef { source: "base/code-reviewer".to_string(), version: "v1".to_string() };
        let layer = resolver.resolve(&layer_ref).unwrap();
        assert_eq!(layer.meta.version, "v1.1",
            "major version 'v1' should resolve to latest v1.x, not v2.0");
    }

    #[test]
    fn test_resolve_major_version_no_false_prefix_match() {
        let tmp = TempDir::new().unwrap();
        // "v1" must NOT match "v10" or "v11".
        // Use a unique name that cannot exist in the global layers cache.
        create_test_layer(tmp.path(), "prefix-test-unique", "test", "v10.0");
        create_test_layer(tmp.path(), "prefix-test-unique", "test", "v11.0");

        // Only search the tmp directory (no global layers), so global state can't interfere.
        let resolver = LayerResolver { extra_paths: vec![tmp.path().to_path_buf()] };
        let layer_ref = LayerRef { source: "test/prefix-test-unique".to_string(), version: "v1".to_string() };
        // No v1.x versions exist; should return an error (not v10/v11)
        assert!(resolver.resolve(&layer_ref).is_err(),
            "major version 'v1' must not match 'v10' or 'v11'");
    }

    #[test]
    fn test_parse_semver_handles_v_prefix() {
        assert_eq!(parse_semver("v1.0.0"), Some(semver::Version::new(1, 0, 0)));
        assert_eq!(parse_semver("v1.2.3"), Some(semver::Version::new(1, 2, 3)));
    }

    #[test]
    fn test_parse_semver_handles_two_part_version() {
        // "v1.9" should be treated as "1.9.0"
        assert_eq!(parse_semver("v1.9"), Some(semver::Version::new(1, 9, 0)));
        assert_eq!(parse_semver("1.10"), Some(semver::Version::new(1, 10, 0)));
    }

    #[test]
    fn test_parse_semver_returns_none_for_invalid() {
        assert_eq!(parse_semver("not-a-version"), None);
        assert_eq!(parse_semver(""), None);
    }

    #[test]
    fn test_search_layers_by_name() {
        let tmp = TempDir::new().unwrap();

        create_test_layer(tmp.path(), "reviewer", "base", "v1.0");
        create_test_layer(tmp.path(), "translator", "base", "v1.0");

        let results = super::search_layers(&[tmp.path().to_path_buf()], "reviewer");
        assert_eq!(results.len(), 1, "should find exactly one matching layer");
        assert!(results[0].0.contains("reviewer"), "result name should contain keyword");
    }

    #[test]
    fn test_search_layers_no_match() {
        let tmp = TempDir::new().unwrap();
        create_test_layer(tmp.path(), "reviewer", "base", "v1.0");

        let results = super::search_layers(&[tmp.path().to_path_buf()], "nonexistent-keyword");
        assert!(results.is_empty(), "no results expected for unmatched keyword");
    }

    #[test]
    fn test_search_layers_empty_dirs() {
        // Searching in an empty dirs list should return an empty result.
        let results = super::search_layers(&[], "anything");
        assert!(results.is_empty(), "empty dir list should yield empty results");
    }

    #[test]
    fn test_search_layers_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        create_test_layer(tmp.path(), "Reviewer", "base", "v1.0");

        // lowercase search should still find the layer
        let results = super::search_layers(&[tmp.path().to_path_buf()], "reviewer");
        assert!(!results.is_empty(), "search should be case-insensitive");
    }

    #[test]
    fn test_search_layers_searches_multiple_dirs() {
        // Results from all directories are aggregated into a single list.
        let tmp1 = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();

        create_test_layer(tmp1.path(), "reviewer", "base", "v1.0");
        create_test_layer(tmp2.path(), "translator", "base", "v1.0");

        let results = super::search_layers(
            &[tmp1.path().to_path_buf(), tmp2.path().to_path_buf()],
            "base",
        );
        assert_eq!(results.len(), 2,
            "layers from both directories should be included in results");
    }

    #[test]
    fn test_parse_semver_three_part_no_prefix() {
        // Three-part versions without a leading 'v' should be parsed correctly.
        assert_eq!(parse_semver("1.2.3"), Some(semver::Version::new(1, 2, 3)));
        assert_eq!(parse_semver("0.1.0"), Some(semver::Version::new(0, 1, 0)));
    }

    #[test]
    fn test_resolve_with_requires_loads_base_layers() {
        let tmp = TempDir::new().unwrap();

        // Create base layer pua/core/v1.0
        let core_dir = tmp.path().join("pua").join("core").join("v1.0");
        fs::create_dir_all(&core_dir).unwrap();
        fs::write(core_dir.join("layer.yaml"), "name: core\nnamespace: pua\nversion: v1.0\ndescription: \"PUA core\"\nauthor: test\ntags: []\nsections: [instructions]\nconflicts: []\nrequires: []\nmodels: []\n").unwrap();
        fs::write(core_dir.join("prompt.md"), "[instructions]\nCore methodology.\n").unwrap();

        // Create dependent layer pua/zh/v1.0 that requires pua/core
        let zh_dir = tmp.path().join("pua").join("zh").join("v1.0");
        fs::create_dir_all(&zh_dir).unwrap();
        fs::write(zh_dir.join("layer.yaml"), "name: zh\nnamespace: pua\nversion: v1.0\ndescription: \"PUA Chinese\"\nauthor: test\ntags: []\nsections: [rhetoric]\nconflicts: []\nrequires: [\"pua/core:v1.0\"]\nmodels: []\n").unwrap();
        fs::write(zh_dir.join("prompt.md"), "[rhetoric]\nChinese PUA rhetoric.\n").unwrap();

        let resolver = LayerResolver::new(vec![tmp.path().to_path_buf()]);
        let layer_ref = LayerRef { source: "pua/zh".to_string(), version: "v1.0".to_string() };

        let (layer, deps) = resolver.resolve_with_requires(&layer_ref).unwrap();
        assert_eq!(layer.meta.name, "zh");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].meta.name, "core");
    }

    #[test]
    fn test_resolve_with_requires_deduplicates_shared_deps() {
        // A → B, A → C, B → D, C → D — D should appear only once
        let tmp = TempDir::new().unwrap();

        let make = |ns: &str, name: &str, requires: &[&str]| {
            let dir = tmp.path().join(ns).join(name).join("v1.0");
            fs::create_dir_all(&dir).unwrap();
            let req_list: Vec<String> = requires.iter().map(|r| format!("\"{}\"", r)).collect();
            let yaml = format!(
                "name: {name}\nnamespace: {ns}\nversion: v1.0\ndescription: \"\"\nauthor: test\ntags: []\nsections: []\nconflicts: []\nrequires: [{reqs}]\nmodels: []\n",
                name=name, ns=ns, reqs=req_list.join(", ")
            );
            fs::write(dir.join("layer.yaml"), yaml).unwrap();
            fs::write(dir.join("prompt.md"), "").unwrap();
        };

        make("t", "d", &[]);
        make("t", "b", &["t/d:v1.0"]);
        make("t", "c", &["t/d:v1.0"]);
        make("t", "a", &["t/b:v1.0", "t/c:v1.0"]);

        let resolver = LayerResolver::new(vec![tmp.path().to_path_buf()]);
        let layer_ref = LayerRef { source: "t/a".to_string(), version: "v1.0".to_string() };
        let (_, deps) = resolver.resolve_with_requires(&layer_ref).unwrap();

        let names: Vec<&str> = deps.iter().map(|l| l.meta.name.as_str()).collect();
        assert_eq!(names.iter().filter(|&&n| n == "d").count(), 1, "d should appear once: {:?}", names);
    }

    #[test]
    fn test_resolve_with_requires_cycle_returns_error() {
        let tmp = TempDir::new().unwrap();

        // Create A that requires B, B that requires A (cycle)
        let make = |name: &str, requires: &[&str]| {
            let dir = tmp.path().join("cycle").join(name).join("v1.0");
            std::fs::create_dir_all(&dir).unwrap();
            let req_list: Vec<String> = requires.iter().map(|r| format!("\"{}\"", r)).collect();
            let yaml = format!(
                "name: {name}\nnamespace: cycle\nversion: v1.0\ndescription: \"\"\nauthor: test\ntags: []\nsections: []\nconflicts: []\nrequires: [{reqs}]\nmodels: []\n",
                name=name, reqs=req_list.join(", ")
            );
            std::fs::write(dir.join("layer.yaml"), yaml).unwrap();
            std::fs::write(dir.join("prompt.md"), "").unwrap();
        };

        make("a", &["cycle/b:v1.0"]);
        make("b", &["cycle/a:v1.0"]);

        let resolver = LayerResolver::new(vec![tmp.path().to_path_buf()]);
        let layer_ref = LayerRef { source: "cycle/a".to_string(), version: "v1.0".to_string() };
        let result = resolver.resolve_with_requires(&layer_ref);
        assert!(result.is_err(), "circular dependency should return an error");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("ircular") || err_msg.contains("cycle") || err_msg.contains("cycle/a") || err_msg.contains("cycle/b"),
            "error message should mention the cycle: {}", err_msg);
    }
}
