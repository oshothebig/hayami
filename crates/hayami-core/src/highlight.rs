//! Syntax highlighting with tree-sitter.
//!
//! Each side of a file diff is highlighted as a whole file and the result is
//! split into per-line spans. Diff lines then look up their spans by line
//! number, so hunks starting inside a string or block comment keep correct
//! colors.

use std::ops::Range;
use std::sync::OnceLock;

use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::diff::SourceText;

/// Files larger than this are not highlighted.
const MAX_HIGHLIGHT_BYTES: usize = 2 * 1024 * 1024;

/// Bundled languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    /// Rust.
    Rust,
    /// TypeScript.
    TypeScript,
    /// TSX.
    Tsx,
    /// JavaScript (including JSX).
    JavaScript,
    /// Python.
    Python,
    /// Go.
    Go,
    /// JSON.
    Json,
    /// YAML.
    Yaml,
    /// TOML.
    Toml,
    /// Markdown (block structure).
    Markdown,
}

impl Language {
    /// All bundled languages.
    pub const ALL: [Language; 10] = [
        Language::Rust,
        Language::TypeScript,
        Language::Tsx,
        Language::JavaScript,
        Language::Python,
        Language::Go,
        Language::Json,
        Language::Yaml,
        Language::Toml,
        Language::Markdown,
    ];

    /// Detects the language from a file path's extension.
    pub fn from_path(path: &str) -> Option<Self> {
        let name = path.rsplit('/').next().unwrap_or(path);
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
        Some(match ext.as_str() {
            "rs" => Language::Rust,
            "ts" | "mts" | "cts" => Language::TypeScript,
            "tsx" => Language::Tsx,
            "js" | "mjs" | "cjs" | "jsx" => Language::JavaScript,
            "py" | "pyi" => Language::Python,
            "go" => Language::Go,
            "json" => Language::Json,
            "yaml" | "yml" => Language::Yaml,
            "toml" => Language::Toml,
            "md" | "markdown" => Language::Markdown,
            _ => return None,
        })
    }

    fn build_config(self) -> Result<HighlightConfiguration, String> {
        let js = tree_sitter_javascript::HIGHLIGHT_QUERY;
        let jsx = tree_sitter_javascript::JSX_HIGHLIGHT_QUERY;
        let ts = tree_sitter_typescript::HIGHLIGHTS_QUERY;
        let (language, highlights): (tree_sitter::Language, String) = match self {
            Language::Rust => (
                tree_sitter_rust::LANGUAGE.into(),
                tree_sitter_rust::HIGHLIGHTS_QUERY.into(),
            ),
            Language::TypeScript => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                format!("{ts}\n{js}"),
            ),
            Language::Tsx => (
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                format!("{ts}\n{js}\n{jsx}"),
            ),
            Language::JavaScript => (
                tree_sitter_javascript::LANGUAGE.into(),
                format!("{js}\n{jsx}"),
            ),
            Language::Python => (
                tree_sitter_python::LANGUAGE.into(),
                tree_sitter_python::HIGHLIGHTS_QUERY.into(),
            ),
            Language::Go => (
                tree_sitter_go::LANGUAGE.into(),
                tree_sitter_go::HIGHLIGHTS_QUERY.into(),
            ),
            Language::Json => (
                tree_sitter_json::LANGUAGE.into(),
                tree_sitter_json::HIGHLIGHTS_QUERY.into(),
            ),
            Language::Yaml => (
                tree_sitter_yaml::LANGUAGE.into(),
                tree_sitter_yaml::HIGHLIGHTS_QUERY.into(),
            ),
            Language::Toml => (
                tree_sitter_toml_ng::LANGUAGE.into(),
                tree_sitter_toml_ng::HIGHLIGHTS_QUERY.into(),
            ),
            Language::Markdown => (
                tree_sitter_md::LANGUAGE.into(),
                tree_sitter_md::HIGHLIGHT_QUERY_BLOCK.into(),
            ),
        };
        let mut config =
            HighlightConfiguration::new(language, format!("{self:?}"), &highlights, "", "")
                .map_err(|e| format!("{self:?} highlight query: {e}"))?;
        config.configure(CAPTURE_NAMES);
        Ok(config)
    }

    /// Shared configuration, built on first use. `None` if the bundled query
    /// failed to compile (a bug, covered by tests).
    fn config(self) -> Option<&'static HighlightConfiguration> {
        static CONFIGS: [OnceLock<Option<HighlightConfiguration>>; 10] =
            [const { OnceLock::new() }; 10];
        let index = Language::ALL.iter().position(|l| *l == self)?;
        CONFIGS[index]
            .get_or_init(|| self.build_config().ok())
            .as_ref()
    }
}

/// Coarse highlight categories that the UI maps to colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HighlightKind {
    /// Attributes and annotations.
    Attribute,
    /// Comments.
    Comment,
    /// Constants and booleans.
    Constant,
    /// Function names.
    Function,
    /// Keywords.
    Keyword,
    /// Numbers.
    Number,
    /// Operators.
    Operator,
    /// Properties and fields (also object keys).
    Property,
    /// Punctuation.
    Punctuation,
    /// Strings.
    String,
    /// Escape sequences and special strings.
    Escape,
    /// Markup tags.
    Tag,
    /// Types and modules.
    Type,
    /// Built-in variables such as `self`.
    VariableBuiltin,
    /// Markdown headings.
    Title,
    /// Links and literal markup.
    Link,
}

/// Capture names recognized from the bundled queries, and their category.
const CAPTURES: &[(&str, HighlightKind)] = &[
    ("attribute", HighlightKind::Attribute),
    ("comment", HighlightKind::Comment),
    ("constant", HighlightKind::Constant),
    ("boolean", HighlightKind::Constant),
    ("constructor", HighlightKind::Type),
    ("function", HighlightKind::Function),
    ("function.macro", HighlightKind::Function),
    ("keyword", HighlightKind::Keyword),
    ("label", HighlightKind::Keyword),
    ("number", HighlightKind::Number),
    ("float", HighlightKind::Number),
    ("operator", HighlightKind::Operator),
    ("property", HighlightKind::Property),
    ("punctuation", HighlightKind::Punctuation),
    ("string", HighlightKind::String),
    ("string.escape", HighlightKind::Escape),
    ("string.special", HighlightKind::Escape),
    ("escape", HighlightKind::Escape),
    ("tag", HighlightKind::Tag),
    ("type", HighlightKind::Type),
    ("module", HighlightKind::Type),
    ("variable.builtin", HighlightKind::VariableBuiltin),
    ("text.title", HighlightKind::Title),
    ("markup.heading", HighlightKind::Title),
    ("text.literal", HighlightKind::String),
    ("text.uri", HighlightKind::Link),
    ("text.reference", HighlightKind::Link),
];

const CAPTURE_NAMES: &[&str] = &{
    let mut names = [""; CAPTURES.len()];
    let mut i = 0;
    while i < CAPTURES.len() {
        names[i] = CAPTURES[i].0;
        i += 1;
    }
    names
};

/// A highlighted byte range within one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Byte range relative to the line start.
    pub range: Range<usize>,
    /// Category.
    pub kind: HighlightKind,
}

/// Per-line spans of one file version, indexed by line.
pub type LineSpans = Vec<Vec<Span>>;

/// Highlights `text` as a whole. Returns `None` for unsupported languages,
/// oversized files or parse failures.
pub fn highlight(language: Language, text: &SourceText) -> Option<LineSpans> {
    let source = text.as_str();
    if source.len() > MAX_HIGHLIGHT_BYTES {
        return None;
    }
    let config = language.config()?;
    let mut highlighter = Highlighter::new();
    let events = highlighter
        .highlight(config, source.as_bytes(), None, None, |_| None)
        .ok()?;

    let mut lines: LineSpans = vec![Vec::new(); text.line_count()];
    let mut stack: Vec<HighlightKind> = Vec::new();
    let mut line = 0;
    for event in events {
        match event.ok()? {
            HighlightEvent::HighlightStart(h) => {
                stack.push(
                    CAPTURES
                        .get(h.0)
                        .map_or(HighlightKind::Punctuation, |c| c.1),
                );
            }
            HighlightEvent::HighlightEnd => {
                stack.pop();
            }
            HighlightEvent::Source { start, end } => {
                let Some(&kind) = stack.last() else {
                    continue;
                };
                // Split the source range at line boundaries.
                while line < lines.len() {
                    let range = text.line_range(line);
                    let line_start = range.start;
                    let next_start = if line + 1 < lines.len() {
                        text.line_range(line + 1).start
                    } else {
                        usize::MAX
                    };
                    let s = start.max(range.start);
                    let e = end.min(range.end);
                    if s < e {
                        push_span(&mut lines[line], s - line_start..e - line_start, kind);
                    }
                    if end <= next_start {
                        break;
                    }
                    line += 1;
                }
            }
        }
    }
    Some(lines)
}

/// Appends a span, merging with the previous one when adjacent and equal.
fn push_span(spans: &mut Vec<Span>, range: Range<usize>, kind: HighlightKind) {
    if let Some(last) = spans.last_mut()
        && last.kind == kind
        && last.range.end == range.start
    {
        last.range.end = range.end;
        return;
    }
    spans.push(Span { range, kind });
}

/// Highlights of both versions of one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileHighlights {
    /// Old version, if highlighted.
    pub old: Option<LineSpans>,
    /// New version, if highlighted.
    pub new: Option<LineSpans>,
}

/// Highlights both sides of a file diff. Cheap no-op for binary files and
/// unsupported languages.
pub fn highlight_file(file: &crate::diff::FileDiff) -> FileHighlights {
    use crate::diff::{FileContent, FileStatus};
    let FileContent::Text { old, new, .. } = &file.content else {
        return FileHighlights::default();
    };
    let old_path = match &file.status {
        FileStatus::Renamed { from } => from.as_str(),
        _ => file.path.as_str(),
    };
    let side = |path: &str, text: &SourceText| {
        Language::from_path(path).and_then(|lang| {
            (text.line_count() > 0)
                .then(|| highlight(lang, text))
                .flatten()
        })
    };
    FileHighlights {
        old: side(old_path, old),
        new: side(&file.path, new),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds_at(spans: &[Span], line: &str, needle: &str) -> Vec<HighlightKind> {
        let at = line.find(needle).unwrap();
        spans
            .iter()
            .filter(|s| s.range.start <= at && at < s.range.end)
            .map(|s| s.kind)
            .collect()
    }

    #[test]
    fn all_bundled_queries_compile() {
        for lang in Language::ALL {
            lang.build_config().unwrap();
            assert!(lang.config().is_some(), "{lang:?}");
        }
    }

    #[test]
    fn detects_languages() {
        assert_eq!(Language::from_path("src/main.rs"), Some(Language::Rust));
        assert_eq!(Language::from_path("a/b.tsx"), Some(Language::Tsx));
        assert_eq!(Language::from_path("x.YML"), Some(Language::Yaml));
        assert_eq!(Language::from_path("Cargo.toml"), Some(Language::Toml));
        assert_eq!(Language::from_path("README.md"), Some(Language::Markdown));
        assert_eq!(Language::from_path("Makefile"), None);
        assert_eq!(Language::from_path("a.c"), None);
    }

    #[test]
    fn highlights_rust_keywords_strings_and_comments() {
        let text = SourceText::new("fn main() {\n    let s = \"hi\"; // note\n}\n".into());
        let lines = highlight(Language::Rust, &text).unwrap();
        assert_eq!(lines.len(), 3);
        assert!(kinds_at(&lines[0], text.line(0), "fn").contains(&HighlightKind::Keyword));
        assert!(kinds_at(&lines[1], text.line(1), "hi").contains(&HighlightKind::String));
        assert!(kinds_at(&lines[1], text.line(1), "note").contains(&HighlightKind::Comment));
    }

    #[test]
    fn multiline_block_comment_covers_inner_lines() {
        let text =
            SourceText::new("fn a() {}\n/* start\n   let x = 1;\n   end */\nfn b() {}\n".into());
        let lines = highlight(Language::Rust, &text).unwrap();
        // The middle line would be code if highlighted on its own.
        assert_eq!(
            lines[2],
            vec![Span {
                range: 0..text.line(2).len(),
                kind: HighlightKind::Comment
            }]
        );
        assert!(kinds_at(&lines[4], text.line(4), "fn").contains(&HighlightKind::Keyword));
    }

    #[test]
    fn each_language_produces_spans() {
        let samples = [
            (Language::TypeScript, "const x: number = 1;\n"),
            (Language::Tsx, "const a = <div className=\"x\" />;\n"),
            (Language::JavaScript, "function f() { return 'a'; }\n"),
            (Language::Python, "def f():\n    return \"a\"\n"),
            (
                Language::Go,
                "package main\nfunc f() string { return \"a\" }\n",
            ),
            (Language::Json, "{\"a\": 1}\n"),
            (Language::Yaml, "key: value\n"),
            (Language::Toml, "[package]\nname = \"x\"\n"),
            (Language::Markdown, "# Title\n\ntext\n"),
        ];
        for (lang, src) in samples {
            let text = SourceText::new(src.into());
            let lines = highlight(lang, &text).unwrap();
            assert!(lines.iter().any(|l| !l.is_empty()), "{lang:?}: {lines:?}");
        }
    }
}
