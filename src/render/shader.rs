//! Source preparation for Wallpaper Engine shaders.
//!
//! This module expands literal includes, reads shader annotations and inserts
//! integer combo definitions. It does not evaluate conditional directives,
//! expand other macros or translate Wallpaper Engine's shader dialect to WGSL.
//! Includes in conditional branches are therefore resolved even when a later
//! compiler would discard that branch. GPU compilation belongs to the renderer.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::project::assets::{AssetStore, normalize_path};
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

const MAX_INCLUDE_DEPTH: usize = 64;
const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct PreparedShader {
    pub(crate) source: String,
    /// Files in first-read order, including the entry file.
    pub(crate) dependencies: Vec<String>,
    pub(crate) combos: BTreeMap<String, i32>,
    pub(crate) annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnnotationKind {
    Combo,
    Parameter,
}

#[derive(Debug, Clone)]
pub(crate) struct Annotation {
    pub(crate) kind: AnnotationKind,
    pub(crate) file: String,
    /// One-based line number in the original source file.
    pub(crate) line: usize,
    /// The declared type and name when the annotation follows a uniform.
    pub(crate) uniform: Option<UniformDeclaration>,
    pub(crate) metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UniformDeclaration {
    pub(crate) ty: String,
    pub(crate) name: String,
}

pub(crate) struct ShaderPreprocessor {
    assets: Arc<AssetStore>,
}

impl ShaderPreprocessor {
    pub(crate) fn new(assets: Arc<AssetStore>) -> Self {
        Self { assets }
    }

    pub(crate) fn preprocess(
        &self,
        path: &str,
        overrides: &BTreeMap<String, i32>,
    ) -> Result<PreparedShader> {
        for name in overrides.keys() {
            ensure!(is_identifier(name), "invalid shader combo name: {name:?}");
        }
        let path = normalize_path(path)?;
        let mut expansion = Expansion {
            assets: &self.assets,
            stack: Vec::new(),
            dependencies: Vec::new(),
            seen: BTreeSet::new(),
            annotations: Vec::new(),
            bytes: 0,
        };
        let source = expansion.expand(&path)?;
        let mut combos = BTreeMap::new();
        for annotation in &expansion.annotations {
            if annotation.kind != AnnotationKind::Combo {
                continue;
            }
            let name = annotation
                .metadata
                .get("combo")
                .and_then(Value::as_str)
                .with_context(|| {
                    format!(
                        "combo annotation has no name at {}:{}",
                        annotation.file, annotation.line
                    )
                })?;
            ensure!(
                is_identifier(name),
                "invalid shader combo name {name:?} at {}:{}",
                annotation.file,
                annotation.line
            );
            if overrides.contains_key(name) {
                continue;
            }
            let default = match annotation.metadata.get("default") {
                Some(value) => i32::try_from(
                    value
                        .as_i64()
                        .with_context(|| format!("combo {name} default must be an integer"))?,
                )
                .with_context(|| format!("combo {name} default is outside i32 range"))?,
                None => 0,
            };
            if let Some(previous) = combos.insert(name.to_owned(), default) {
                ensure!(
                    previous == default,
                    "conflicting defaults for shader combo {name}"
                );
            }
        }
        combos.extend(overrides.iter().map(|(k, v)| (k.clone(), *v)));
        let mut definitions = String::new();
        for (name, value) in &combos {
            definitions.push_str(&format!("#define {name} {value}\n"));
        }
        let source = insert_definitions(&source, &definitions);
        ensure!(
            source.len() <= MAX_SOURCE_BYTES,
            "expanded shader exceeds 8 MiB"
        );
        Ok(PreparedShader {
            source,
            dependencies: expansion.dependencies,
            combos,
            annotations: expansion.annotations,
        })
    }
}

struct Expansion<'a> {
    assets: &'a AssetStore,
    stack: Vec<String>,
    dependencies: Vec<String>,
    seen: BTreeSet<String>,
    annotations: Vec<Annotation>,
    bytes: usize,
}

impl Expansion<'_> {
    fn expand(&mut self, path: &str) -> Result<String> {
        ensure!(
            self.stack.len() < MAX_INCLUDE_DEPTH,
            "shader include depth exceeds {MAX_INCLUDE_DEPTH}"
        );
        if self.stack.iter().any(|entry| entry == path) {
            bail!(
                "shader include cycle: {} -> {path}",
                self.stack.join(" -> ")
            );
        }
        let bytes = self
            .assets
            .read(path)
            .with_context(|| format!("cannot read shader {path}"))?;
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .context("shader source size overflow")?;
        ensure!(
            self.bytes <= MAX_SOURCE_BYTES,
            "shader includes exceed 8 MiB"
        );
        let source =
            String::from_utf8(bytes).with_context(|| format!("shader is not UTF-8: {path}"))?;
        if self.seen.insert(path.to_owned()) {
            self.dependencies.push(path.to_owned());
        }
        self.stack.push(path.to_owned());
        let mut result = String::new();
        let mut in_block_comment = false;
        for (index, line) in source.lines().enumerate() {
            let (code, comment) = split_comments(line, &mut in_block_comment);
            let code = code.trim();
            self.read_annotation(path, index + 1, code, comment)?;
            if let Some(directive) = directive_body(code, "include") {
                let include = parse_include(directive)
                    .with_context(|| format!("invalid include at {path}:{}", index + 1))?;
                let resolved = self.resolve(path, include)?;
                result.push_str(&self.expand(&resolved)?);
            } else {
                result.push_str(line);
                result.push('\n');
            }
            ensure!(
                result.len() <= MAX_SOURCE_BYTES,
                "expanded shader exceeds 8 MiB"
            );
        }
        self.stack.pop();
        Ok(result)
    }

    fn resolve(&self, parent: &str, include: &str) -> Result<String> {
        ensure!(
            !include.starts_with('/') && !include.contains([':', '\0']),
            "invalid shader include path: {include:?}"
        );
        let include = include.replace('\\', "/");
        let directory = parent.rsplit_once('/').map_or("", |(dir, _)| dir);
        let mut candidates = Vec::new();
        for root in [directory, "shaders/include", "shaders", ""] {
            match relative_path(root, &include) {
                Ok(candidate) if !candidates.contains(&candidate) => candidates.push(candidate),
                _ => {}
            }
        }
        for candidate in &candidates {
            if self.assets.exists(candidate) {
                return Ok(candidate.clone());
            }
        }
        bail!(
            "shader include {include:?} from {parent} not found; tried {}",
            candidates.join(", ")
        )
    }

    fn read_annotation(
        &mut self,
        path: &str,
        line: usize,
        code: &str,
        comment: Option<&str>,
    ) -> Result<()> {
        let Some(comment) = comment.map(str::trim) else {
            return Ok(());
        };
        let uniform = parse_uniform(code);
        let (kind, json) = if let Some(json) = comment.strip_prefix("[COMBO]") {
            (AnnotationKind::Combo, json.trim())
        } else if let Some(json) = comment.strip_prefix("[PARAMETER]") {
            (AnnotationKind::Parameter, json.trim())
        } else if uniform.is_some() && comment.starts_with('{') {
            (AnnotationKind::Parameter, comment)
        } else {
            return Ok(());
        };
        let metadata: Value = serde_json::from_str(json)
            .with_context(|| format!("invalid shader annotation at {path}:{line}"))?;
        ensure!(
            metadata.is_object(),
            "shader annotation must be a JSON object at {path}:{line}"
        );
        self.annotations.push(Annotation {
            kind,
            file: path.to_owned(),
            line,
            uniform,
            metadata,
        });
        Ok(())
    }
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn relative_path(directory: &str, path: &str) -> Result<String> {
    let mut parts: Vec<&str> = directory.split('/').filter(|p| !p.is_empty()).collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                ensure!(parts.pop().is_some(), "include escapes asset root");
            }
            _ => parts.push(part),
        }
    }
    normalize_path(&parts.join("/"))
}

fn directive_body<'a>(code: &'a str, directive: &str) -> Option<&'a str> {
    let code = code.strip_prefix('#')?.trim_start();
    let rest = code.strip_prefix(directive)?;
    if rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(rest.trim())
}

fn parse_include(body: &str) -> Result<&str> {
    let (open, close) = if body.starts_with('"') {
        ('"', '"')
    } else {
        ('<', '>')
    };
    ensure!(
        body.starts_with(open),
        "expected a literal quoted or angle-bracket include"
    );
    let end = body[1..]
        .find(close)
        .map(|n| n + 1)
        .context("unterminated include path")?;
    ensure!(
        body[end + 1..].trim().is_empty(),
        "unexpected tokens after include path"
    );
    ensure!(end > 1, "empty include path");
    Ok(&body[1..end])
}

fn parse_uniform(code: &str) -> Option<UniformDeclaration> {
    let declaration = code.strip_prefix("uniform ")?.split_once(';')?.0;
    let mut parts = declaration.split_whitespace();
    let ty = parts.next()?;
    let name = parts.next()?;
    if parts.next().is_some() || !is_identifier(ty) {
        return None;
    }
    let name = name.split('[').next()?;
    if !is_identifier(name) {
        return None;
    }
    Some(UniformDeclaration {
        ty: ty.to_owned(),
        name: name.to_owned(),
    })
}

/// Returns comment-free code and the first actual line comment, while tracking
/// block comments across lines. Quote handling keeps include paths intact.
fn split_comments<'a>(line: &'a str, in_block: &mut bool) -> (String, Option<&'a str>) {
    let mut code = String::new();
    let mut chars = line.char_indices().peekable();
    let mut quoted = false;
    let mut escaped = false;
    while let Some((index, ch)) = chars.next() {
        if *in_block {
            if ch == '*' && chars.peek().is_some_and(|(_, c)| *c == '/') {
                chars.next();
                *in_block = false;
                code.push(' ');
            }
            continue;
        }
        if quoted {
            code.push(ch);
            if !escaped && ch == '"' {
                quoted = false;
            }
            escaped = !escaped && ch == '\\';
            continue;
        }
        if ch == '"' {
            quoted = true;
            code.push(ch);
            continue;
        }
        if ch == '/' {
            if chars.peek().is_some_and(|(_, c)| *c == '/') {
                return (code, Some(&line[index + 2..]));
            }
            if chars.peek().is_some_and(|(_, c)| *c == '*') {
                chars.next();
                *in_block = true;
                code.push(' ');
                continue;
            }
        }
        code.push(ch);
    }
    (code, None)
}

fn insert_definitions(source: &str, definitions: &str) -> String {
    if definitions.is_empty() {
        return source.to_owned();
    }
    let mut in_block = false;
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let (code, _) = split_comments(line, &mut in_block);
        if code.trim().is_empty() {
            offset += line.len();
            continue;
        }
        if directive_body(code.trim(), "version").is_some() {
            offset += line.len();
            return format!("{}{}{}", &source[..offset], definitions, &source[offset..]);
        }
        break;
    }
    format!("{definitions}{source}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new(files: &[(&str, &str)]) -> Self {
            let root = std::env::temp_dir().join(format!(
                "rxwpe-shader-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            for (path, source) in files {
                let path = root.join(path);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, source).unwrap();
            }
            Self(root)
        }
        fn processor(&self) -> ShaderPreprocessor {
            ShaderPreprocessor::new(Arc::new(AssetStore::open(&self.0, None).unwrap()))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn expands_includes_and_reads_combo_and_uniform_annotations() {
        let fixture = Fixture::new(&[
            (
                "shaders/test.frag",
                "#version 330\n#include \"local.h\"\n#include <global.h>\n// [COMBO] {\"combo\":\"BLEND\",\"default\":2}\nuniform float g_Alpha; // {\"material\":\"alpha\",\"default\":1}\nvoid main() {}",
            ),
            ("shaders/local.h", "#include \"nested.h\"\nfloat local;"),
            ("shaders/nested.h", "float nested;"),
            (
                "shaders/include/global.h",
                "// [COMBO] {\"combo\":\"GLOBAL\"}\nfloat global;",
            ),
        ]);
        let prepared = fixture
            .processor()
            .preprocess("shaders/test.frag", &BTreeMap::from([("BLEND".into(), 3)]))
            .unwrap();
        assert!(
            prepared
                .source
                .starts_with("#version 330\n#define BLEND 3\n#define GLOBAL 0\n")
        );
        assert!(prepared.source.contains("float nested;\nfloat local;\n"));
        assert!(!prepared.source.contains("#include"));
        assert_eq!(
            prepared.dependencies,
            [
                "shaders/test.frag",
                "shaders/local.h",
                "shaders/nested.h",
                "shaders/include/global.h"
            ]
        );
        let parameter = prepared
            .annotations
            .iter()
            .find(|a| a.kind == AnnotationKind::Parameter)
            .unwrap();
        assert_eq!(parameter.uniform.as_ref().unwrap().name, "g_Alpha");
        assert_eq!(parameter.metadata["material"], "alpha");
    }

    #[test]
    fn ignores_commented_directives_and_annotations() {
        let fixture = Fixture::new(&[(
            "main.frag",
            "/*\n#include \"missing.h\"\n// [COMBO] bad json\n*/\n// #include \"also-missing.h\"\nvoid main() {}",
        )]);
        let prepared = fixture
            .processor()
            .preprocess("main.frag", &BTreeMap::new())
            .unwrap();
        assert!(prepared.annotations.is_empty());
        assert_eq!(prepared.dependencies.len(), 1);
    }

    #[test]
    fn reports_cycles_and_missing_includes_with_context() {
        let fixture = Fixture::new(&[
            ("a.h", "#include \"b.h\""),
            ("b.h", "#include \"a.h\""),
            ("missing.frag", "#include \"absent.h\""),
        ]);
        let error = fixture
            .processor()
            .preprocess("a.h", &BTreeMap::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("a.h -> b.h -> a.h"), "{error}");
        let error = fixture
            .processor()
            .preprocess("missing.frag", &BTreeMap::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("absent.h") && error.contains("missing.frag"));
    }

    #[test]
    fn rejects_invalid_combo_names_and_non_integer_defaults() {
        let fixture = Fixture::new(&[("a.frag", "// [COMBO] {\"combo\":\"BAD\",\"default\":1.5}")]);
        assert!(
            fixture
                .processor()
                .preprocess("a.frag", &BTreeMap::new())
                .is_err()
        );
        assert!(
            fixture
                .processor()
                .preprocess("a.frag", &BTreeMap::from([("A\n#define B".into(), 1)]))
                .is_err()
        );
    }
}
