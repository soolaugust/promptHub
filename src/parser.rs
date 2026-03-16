use std::collections::HashMap;
use std::path::PathBuf;

/// Reference to a layer with version
#[derive(Debug, Clone, PartialEq)]
pub struct LayerRef {
    /// Namespace/name, e.g. "base/code-reviewer"
    pub source: String,
    /// Version string, e.g. "v1.0", "latest", ""
    pub version: String,
}

impl LayerRef {
    pub fn parse(input: &str) -> crate::error::Result<Self> {
        let parts: Vec<&str> = input.splitn(2, ':').collect();
        let source = parts[0].trim().to_string();
        let version = if parts.len() > 1 {
            parts[1].trim().to_string()
        } else {
            "latest".to_string()
        };
        if source.is_empty() {
            return Err(crate::error::PromptHubError::ParseError(
                format!("Empty layer reference: {}", input)
            ));
        }
        Ok(LayerRef { source, version })
    }

    pub fn display(&self) -> String {
        if self.version.is_empty() || self.version == "latest" {
            self.source.clone()
        } else {
            format!("{}:{}", self.source, self.version)
        }
    }
}

/// Parsed representation of a Promptfile
#[derive(Debug, Clone)]
pub struct Promptfile {
    pub from: LayerRef,
    pub layers: Vec<LayerRef>,
    pub params: HashMap<String, String>,
    pub vars: HashMap<String, String>,
    pub task: Option<String>,
    pub includes: Vec<PathBuf>,
}

/// A single instruction parsed from a Promptfile
#[derive(Debug, Clone)]
pub enum Instruction {
    From(LayerRef),
    Layer(LayerRef),
    Param(String, String),
    Var(String, String),
    Task(String),
    Include(PathBuf),
    Comment(String),
}

/// Parse a Promptfile from string content
pub fn parse(content: &str) -> crate::error::Result<Promptfile> {
    let instructions = parse_instructions(content)?;
    build_promptfile(instructions)
}

fn parse_instructions(content: &str) -> crate::error::Result<Vec<Instruction>> {
    let mut instructions = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(stripped) = line.strip_prefix('#') {
            instructions.push(Instruction::Comment(stripped.trim().to_string()));
            continue;
        }

        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        let directive = parts[0].to_uppercase();
        let rest = if parts.len() > 1 { parts[1].trim() } else { "" };

        match directive.as_str() {
            "FROM" => {
                let layer_ref = LayerRef::parse(rest).map_err(|e| {
                    crate::error::PromptHubError::ParseError(
                        format!("Line {}: {}", line_num + 1, e)
                    )
                })?;
                instructions.push(Instruction::From(layer_ref));
            }
            "LAYER" => {
                let layer_ref = LayerRef::parse(rest).map_err(|e| {
                    crate::error::PromptHubError::ParseError(
                        format!("Line {}: {}", line_num + 1, e)
                    )
                })?;
                instructions.push(Instruction::Layer(layer_ref));
            }
            "PARAM" => {
                let (key, value) = parse_key_value(rest, line_num + 1)?;
                instructions.push(Instruction::Param(key, value));
            }
            "VAR" => {
                let (key, value) = parse_key_value(rest, line_num + 1)?;
                instructions.push(Instruction::Var(key, value));
            }
            "TASK" => {
                let value = parse_quoted_string(rest, line_num + 1)?;
                instructions.push(Instruction::Task(value));
            }
            "INCLUDE" => {
                let path = parse_quoted_string(rest, line_num + 1)?;
                instructions.push(Instruction::Include(PathBuf::from(path)));
            }
            _ => {
                return Err(crate::error::PromptHubError::ParseError(
                    format!(
                        "Line {}: Unknown directive '{}'. Valid directives: FROM, LAYER, PARAM, VAR, TASK, INCLUDE",
                        line_num + 1, directive
                    )
                ));
            }
        }
    }

    Ok(instructions)
}

fn parse_key_value(rest: &str, line_num: usize) -> crate::error::Result<(String, String)> {
    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
    if parts.len() < 2 {
        return Err(crate::error::PromptHubError::ParseError(
            format!("Line {}: Expected key and value", line_num)
        ));
    }
    let key = parts[0].to_string();
    let value = parse_quoted_string(parts[1].trim(), line_num)?;
    Ok((key, value))
}

fn parse_quoted_string(s: &str, line_num: usize) -> crate::error::Result<String> {
    let s = s.trim();
    // Strip surrounding matching quotes (double or single)
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2) ||
       (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2) {
        Ok(s[1..s.len()-1].to_string())
    } else if s.starts_with('"') || s.starts_with('\'') {
        // Opening quote without matching closing quote
        Err(crate::error::PromptHubError::ParseError(
            format!("Line {}: Mismatched quote in: {:?}", line_num, s)
        ))
    } else if !s.is_empty() {
        // Allow unquoted values
        Ok(s.to_string())
    } else {
        Err(crate::error::PromptHubError::ParseError(
            format!("Line {}: Expected a value", line_num)
        ))
    }
}

fn build_promptfile(instructions: Vec<Instruction>) -> crate::error::Result<Promptfile> {
    let mut from: Option<LayerRef> = None;
    let mut layers = Vec::new();
    let mut params = HashMap::new();
    let mut vars = HashMap::new();
    let mut task: Option<String> = None;
    let mut includes = Vec::new();

    for inst in instructions {
        match inst {
            Instruction::From(layer_ref) => {
                if from.is_some() {
                    return Err(crate::error::PromptHubError::ParseError(
                        "Multiple FROM directives found; only one is allowed".to_string()
                    ));
                }
                from = Some(layer_ref);
            }
            Instruction::Layer(layer_ref) => layers.push(layer_ref),
            Instruction::Param(k, v) => { params.insert(k, v); }
            Instruction::Var(k, v) => { vars.insert(k, v); }
            Instruction::Task(t) => { task = Some(t); }
            Instruction::Include(p) => includes.push(p),
            Instruction::Comment(_) => {}
        }
    }

    let from = from.ok_or_else(|| {
        crate::error::PromptHubError::ParseError(
            "Missing FROM directive; Promptfile must start with FROM".to_string()
        )
    })?;

    Ok(Promptfile { from, layers, params, vars, task, includes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic() {
        let content = r#"
FROM base/code-reviewer:v1.0
LAYER style/concise:latest
PARAM model "claude-sonnet-4-6"
VAR language "中文"
TASK "审查这段代码"
"#;
        let pf = parse(content).unwrap();
        assert_eq!(pf.from.source, "base/code-reviewer");
        assert_eq!(pf.from.version, "v1.0");
        assert_eq!(pf.layers.len(), 1);
        assert_eq!(pf.layers[0].source, "style/concise");
        assert_eq!(pf.params.get("model").unwrap(), "claude-sonnet-4-6");
        assert_eq!(pf.vars.get("language").unwrap(), "中文");
        assert_eq!(pf.task.unwrap(), "审查这段代码");
    }

    #[test]
    fn test_parse_multiple_layers() {
        let content = r#"
FROM base/writer:v1.0
LAYER style/academic:v1.0
LAYER lang/english-academic:v1.0
LAYER guard/fact-check:v1.0
"#;
        let pf = parse(content).unwrap();
        assert_eq!(pf.layers.len(), 3);
    }

    #[test]
    fn test_parse_no_from_fails() {
        let content = "LAYER style/concise:latest\n";
        assert!(parse(content).is_err());
    }

    #[test]
    fn test_parse_multiple_from_fails() {
        let content = "FROM base/writer:v1.0\nFROM base/translator:v1.0\n";
        assert!(parse(content).is_err());
    }

    #[test]
    fn test_layer_ref_no_version() {
        let r = LayerRef::parse("base/code-reviewer").unwrap();
        assert_eq!(r.version, "latest");
    }

    #[test]
    fn test_parse_comments_ignored() {
        let content = "# This is a comment\nFROM base/writer:v1.0\n";
        let pf = parse(content).unwrap();
        assert_eq!(pf.from.source, "base/writer");
    }

    #[test]
    fn test_parse_include() {
        let content = "FROM base/writer:v1.0\nINCLUDE ./context.md\n";
        let pf = parse(content).unwrap();
        assert_eq!(pf.includes.len(), 1);
        assert_eq!(pf.includes[0], std::path::PathBuf::from("./context.md"));
    }

    #[test]
    fn test_parse_mismatched_quote_fails() {
        // Opening double-quote without closing quote should be a parse error
        let content = "FROM base/writer:v1.0\nVAR lang \"español\n";
        assert!(parse(content).is_err(), "mismatched quote should produce a parse error");
    }

    #[test]
    fn test_parse_unknown_directive_error_lists_valid() {
        let content = "FROM base/writer:v1.0\nFILTER something\n";
        let err = parse(content).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("FROM") && msg.contains("LAYER"),
            "error message should list valid directives, got: {}", msg);
    }

    #[test]
    fn test_parse_include_quoted() {
        // INCLUDE with quoted path should work
        let content = "FROM base/writer:v1.0\nINCLUDE \"./my context.md\"\n";
        let pf = parse(content).unwrap();
        assert_eq!(pf.includes[0], std::path::PathBuf::from("./my context.md"));
    }

    #[test]
    fn test_parse_include_mismatched_quote_fails() {
        // INCLUDE with mismatched quote should error (same as other directives)
        let content = "FROM base/writer:v1.0\nINCLUDE \"./context.md\n";
        assert!(parse(content).is_err(),
            "INCLUDE with mismatched quote should produce a parse error");
    }
}
