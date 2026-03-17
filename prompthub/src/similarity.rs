use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct Chunk {
    pub heading: String,
    pub body: String,
}

impl Chunk {
    pub fn fingerprint(&self) -> String {
        let normalized: String = self.body.split_whitespace().collect::<Vec<_>>().join(" ");
        let hash = Sha256::digest(normalized.as_bytes());
        hex::encode(&hash[..8])
    }
}

pub fn split_into_chunks(content: &str) -> Vec<Chunk> {
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_body_lines: Vec<String> = Vec::new();

    for line in content.lines() {
        if line.starts_with("## ") {
            if let Some(heading) = current_heading.take() {
                chunks.push(Chunk {
                    heading,
                    body: current_body_lines.join("\n"),
                });
                current_body_lines.clear();
            }
            current_heading = Some(line.trim_start_matches('#').trim().to_string());
        } else if current_heading.is_some() {
            current_body_lines.push(line.to_string());
        }
    }

    if let Some(heading) = current_heading {
        chunks.push(Chunk {
            heading,
            body: current_body_lines.join("\n"),
        });
    }

    chunks
}

pub fn jaccard_similarity(a: &str, b: &str) -> f64 {
    let set_a: HashSet<&str> = a.split_whitespace().collect();
    let set_b: HashSet<&str> = b.split_whitespace().collect();

    if set_a.is_empty() && set_b.is_empty() {
        return 1.0;
    }

    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();

    intersection as f64 / union as f64
}

#[derive(Debug, Clone)]
pub struct SkillContent {
    pub name: String,
    pub chunks: Vec<Chunk>,
}

#[derive(Debug, Clone)]
pub struct CommonChunkSuggestion {
    pub heading: String,
    pub representative_body: String,
    pub skill_names: Vec<String>,
    pub avg_similarity: f64,
}

pub fn find_common_chunks(skills: &[SkillContent], threshold: f64) -> Vec<CommonChunkSuggestion> {
    if skills.len() < 2 {
        return Vec::new();
    }

    let mut results: Vec<CommonChunkSuggestion> = Vec::new();

    // Build fingerprint -> Vec<(skill_name, chunk_idx)> map
    let mut fp_map: HashMap<String, Vec<(String, usize)>> = HashMap::new();
    for skill in skills {
        for (idx, chunk) in skill.chunks.iter().enumerate() {
            fp_map
                .entry(chunk.fingerprint())
                .or_default()
                .push((skill.name.clone(), idx));
        }
    }

    // Track covered pairs to avoid duplicates in approximate matching
    let mut covered_pairs: HashSet<String> = HashSet::new();

    // Exact matches: fingerprint appears in >= 2 different skills
    for (_fp, occurrences) in &fp_map {
        let unique_skills: Vec<&str> = {
            let mut seen: HashSet<&str> = HashSet::new();
            occurrences
                .iter()
                .filter(|(name, _)| seen.insert(name.as_str()))
                .map(|(name, _)| name.as_str())
                .collect()
        };
        if unique_skills.len() < 2 {
            continue;
        }

        // Mark all pairs covered
        let skill_names_sorted: Vec<String> = {
            let mut v: Vec<String> = unique_skills.iter().map(|s| s.to_string()).collect();
            v.sort();
            v
        };
        for i in 0..skill_names_sorted.len() {
            for j in (i + 1)..skill_names_sorted.len() {
                covered_pairs.insert(format!("{}::{}", skill_names_sorted[i], skill_names_sorted[j]));
            }
        }

        // Use the first occurrence's chunk as representative
        let rep_skill_idx = occurrences[0].1;
        let rep_skill = skills
            .iter()
            .find(|s| s.name == occurrences[0].0)
            .expect("skill name from fp_map must exist in skills slice");
        let rep_chunk = &rep_skill.chunks[rep_skill_idx];

        // Collect all distinct skill names (preserving original order of first occurrence)
        let mut seen_names: HashSet<String> = HashSet::new();
        let distinct_skill_names: Vec<String> = occurrences
            .iter()
            .filter(|(name, _)| seen_names.insert(name.clone()))
            .map(|(name, _)| name.clone())
            .collect();

        results.push(CommonChunkSuggestion {
            heading: rep_chunk.heading.clone(),
            representative_body: rep_chunk.body.clone(),
            skill_names: distinct_skill_names,
            avg_similarity: 1.0,
        });

    }

    // Approximate matches: pairs of skills with similar but non-identical chunks
    for i in 0..skills.len() {
        for j in (i + 1)..skills.len() {
            let name_i = &skills[i].name;
            let name_j = &skills[j].name;

            let pair_key = {
                let mut pair = vec![name_i.clone(), name_j.clone()];
                pair.sort();
                format!("{}::{}", pair[0], pair[1])
            };

            if covered_pairs.contains(&pair_key) {
                continue;
            }

            for ci in &skills[i].chunks {
                for cj in &skills[j].chunks {
                    if ci.fingerprint() == cj.fingerprint() {
                        // Already handled by exact match above
                        continue;
                    }
                    let sim = jaccard_similarity(&ci.body, &cj.body);
                    if sim >= threshold {
                        covered_pairs.insert(pair_key.clone());
                        results.push(CommonChunkSuggestion {
                            heading: ci.heading.clone(),
                            representative_body: ci.body.clone(),
                            skill_names: vec![name_i.clone(), name_j.clone()],
                            avg_similarity: sim,
                        });
                        break;
                    }
                }
            }
        }
    }

    // Sort: by skill_names.len() desc, then avg_similarity desc
    results.sort_by(|a, b| {
        b.skill_names
            .len()
            .cmp(&a.skill_names.len())
            .then(b.avg_similarity.partial_cmp(&a.avg_similarity).unwrap_or(std::cmp::Ordering::Equal))
    });

    results
}

/// 一个层拆分建议：把某组 skill 的公共 chunk 抽取为一个 core layer
#[derive(Debug, Clone)]
pub struct SplitPlan {
    /// 受影响的 skill 名称集合
    pub affected_skills: Vec<String>,
    /// 建议的 core layer 名称（如 "pua/core"）
    pub suggested_core_name: String,
    /// 应该进入 core layer 的 chunks
    pub common_chunks: Vec<CommonChunkSuggestion>,
}

/// 将公共 chunk 建议按"受影响 skill 集合"分组，生成层拆分方案
pub fn generate_split_plan(suggestions: &[CommonChunkSuggestion]) -> Vec<SplitPlan> {
    let mut groups: HashMap<String, Vec<CommonChunkSuggestion>> = HashMap::new();
    for s in suggestions {
        let mut sorted = s.skill_names.clone();
        sorted.sort();
        let key = sorted.join(",");
        groups.entry(key).or_default().push(s.clone());
    }

    let mut plans: Vec<SplitPlan> = groups
        .into_iter()
        .map(|(key, chunks)| {
            let affected_skills: Vec<String> = key.split(',').map(String::from).collect();
            let namespace = infer_namespace(&affected_skills);
            SplitPlan {
                affected_skills,
                suggested_core_name: format!("{}/core", namespace),
                common_chunks: chunks,
            }
        })
        .collect();

    plans.sort_by(|a, b| b.affected_skills.len().cmp(&a.affected_skills.len()));
    plans
}

/// 从 skill 名列表推断公共命名空间
/// ["pua", "pua-en", "pua-ja"] → "pua"
/// ["sched-reviewer", "cpu-expert"] → "common"
pub fn infer_namespace(names: &[String]) -> String {
    if names.is_empty() {
        return "common".to_string();
    }
    let first = &names[0];
    for prefix_len in (1..=first.len()).rev() {
        let prefix = &first[..prefix_len];
        if names.iter().all(|n| n.starts_with(prefix)) {
            return prefix.trim_end_matches('-').to_string();
        }
    }
    "common".to_string()
}

/// Build prompt.md content for a core layer from common chunks.
/// Each chunk becomes a [section-name] block.
pub fn extract_core_content(chunks: &[&CommonChunkSuggestion]) -> String {
    let mut parts = Vec::new();
    for chunk in chunks {
        let section = heading_to_section_name(&chunk.heading);
        parts.push(format!("[{}]\n{}", section, chunk.representative_body.trim()));
    }
    parts.join("\n\n")
}

/// Convert a heading string to a section name (lowercase kebab-case for ASCII,
/// sha256-based for non-ASCII like Chinese).
pub fn heading_to_section_name(heading: &str) -> String {
    if heading.is_ascii() {
        heading
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect()
    } else {
        let hash = Sha256::digest(heading.as_bytes());
        format!("section-{}", hex::encode(&hash[..3]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_chunks_by_heading() {
        let content = "## Section A\nContent A here.\n\n## Section B\nContent B here.\n";
        let chunks = split_into_chunks(content);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].heading, "Section A");
        assert!(chunks[0].body.contains("Content A"));
    }

    #[test]
    fn test_chunk_fingerprint_identical_content() {
        let a = Chunk { heading: "H".into(), body: "  hello world  \n".into() };
        let b = Chunk { heading: "H2".into(), body: "hello world".into() };
        assert_eq!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn test_jaccard_identical() {
        assert!((jaccard_similarity("hello world foo", "hello world foo") - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_jaccard_disjoint() {
        assert!((jaccard_similarity("aaa bbb", "ccc ddd") - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_jaccard_partial() {
        let sim = jaccard_similarity("a b c", "a b d");
        assert!((sim - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_find_common_chunks_exact_match() {
        let skill_a = SkillContent {
            name: "pua".into(),
            chunks: vec![
                Chunk { heading: "Rules".into(), body: "Rule one. Rule two.".into() },
                Chunk { heading: "Unique".into(), body: "Only in A.".into() },
            ],
        };
        let skill_b = SkillContent {
            name: "pua-en".into(),
            chunks: vec![
                Chunk { heading: "Rules".into(), body: "Rule one. Rule two.".into() },
                Chunk { heading: "Unique".into(), body: "Only in B.".into() },
            ],
        };
        let suggestions = find_common_chunks(&[skill_a, skill_b], 0.85);
        assert_eq!(suggestions.len(), 1);
        assert!(suggestions[0].skill_names.contains(&"pua".to_string()));
        assert!(suggestions[0].skill_names.contains(&"pua-en".to_string()));
        assert_eq!(suggestions[0].heading, "Rules");
    }

    #[test]
    fn test_find_common_chunks_similar_match() {
        let body_a = "You are a senior P8 engineer with high expectations. Always use this for debugging tasks.";
        let body_b = "You are a senior P8 engineer with high standards. Always use this for debugging tasks.";
        let skill_a = SkillContent {
            name: "pua".into(),
            chunks: vec![Chunk { heading: "Intro".into(), body: body_a.into() }],
        };
        let skill_b = SkillContent {
            name: "pua-en".into(),
            chunks: vec![Chunk { heading: "Intro".into(), body: body_b.into() }],
        };
        let suggestions = find_common_chunks(&[skill_a, skill_b], 0.85);
        assert_eq!(suggestions.len(), 1, "similar chunks should be detected");
    }

    #[test]
    fn test_find_common_chunks_below_threshold() {
        let skill_a = SkillContent {
            name: "pua".into(),
            chunks: vec![Chunk { heading: "X".into(), body: "abc def ghi".into() }],
        };
        let skill_b = SkillContent {
            name: "pua-en".into(),
            chunks: vec![Chunk { heading: "X".into(), body: "xyz uvw rst".into() }],
        };
        let suggestions = find_common_chunks(&[skill_a, skill_b], 0.85);
        assert!(suggestions.is_empty(), "below-threshold should not be suggested");
    }

    #[test]
    fn test_generate_split_plan_groups_by_skill_set() {
        let suggestions = vec![
            CommonChunkSuggestion {
                heading: "Rules".into(),
                representative_body: "Rule content.".into(),
                skill_names: vec!["pua".into(), "pua-en".into(), "pua-ja".into()],
                avg_similarity: 1.0,
            },
            CommonChunkSuggestion {
                heading: "Methodology".into(),
                representative_body: "Method content.".into(),
                skill_names: vec!["pua".into(), "pua-en".into(), "pua-ja".into()],
                avg_similarity: 0.95,
            },
        ];
        let plans = generate_split_plan(&suggestions);
        assert_eq!(plans.len(), 1, "same skill set should produce one core layer");
        assert_eq!(plans[0].common_chunks.len(), 2);
        assert!(plans[0].suggested_core_name.contains("core"));
    }

    #[test]
    fn test_infer_namespace_common_prefix() {
        let names = vec!["pua".into(), "pua-en".into(), "pua-ja".into()];
        assert_eq!(infer_namespace(&names), "pua");
    }

    #[test]
    fn test_infer_namespace_no_common_prefix() {
        let names = vec!["sched-reviewer".into(), "cpu-expert".into()];
        assert_eq!(infer_namespace(&names), "common");
    }

    #[test]
    fn test_extract_core_content_formats_sections() {
        let chunk = CommonChunkSuggestion {
            heading: "Rules".into(),
            representative_body: "Rule one.\nRule two.".into(),
            skill_names: vec!["a".into(), "b".into()],
            avg_similarity: 1.0,
        };
        let content = extract_core_content(&[&chunk]);
        assert!(content.contains("[rules]"), "should have section marker");
        assert!(content.contains("Rule one."), "should have body content");
    }

    #[test]
    fn test_heading_to_section_name_ascii() {
        assert_eq!(heading_to_section_name("Three Iron Rules"), "three-iron-rules");
    }

    #[test]
    fn test_heading_to_section_name_non_ascii() {
        let name = heading_to_section_name("三条铁律");
        assert!(name.starts_with("section-"), "non-ASCII should use hash prefix");
        assert_eq!(name.len(), "section-".len() + 6); // 3 bytes = 6 hex chars
    }
}
