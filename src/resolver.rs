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
}
