//! Language detection and the shared syntax set.
//!
//! Uses bat's extended syntax collection (via `two-face`) which covers ~200
//! languages, plus a table of overrides for extensions that are missing or
//! ambiguous in that collection.

use egui_extras::syntax_highlighting::SyntectSettings;
use std::path::Path;
use std::sync::OnceLock;

pub const PLAIN: &str = "Plain Text";

/// The global syntax/theme set. Must be a stable reference because
/// `egui_extras` memoizes highlighting by the settings' address.
pub fn settings() -> &'static SyntectSettings {
    static S: OnceLock<SyntectSettings> = OnceLock::new();
    S.get_or_init(|| SyntectSettings {
        ps: two_face::syntax::extra_newlines(),
        ts: syntect::highlighting::ThemeSet::load_defaults(),
    })
}

/// Exact (lowercase) file names that need an explicit mapping.
const NAME_OVERRIDES: &[(&str, &str)] = &[
    (".dockerignore", "Git Ignore"),
    (".npmignore", "Git Ignore"),
    (".prettierignore", "Git Ignore"),
    (".eslintignore", "Git Ignore"),
    (".babelrc", "JSON"),
    (".eslintrc", "JSON"),
    (".prettierrc", "JSON"),
    (".swcrc", "JSON"),
    ("tsconfig.json", "JSON"),
    ("justfile", "Makefile"),
    ("containerfile", "Dockerfile"),
    ("license", PLAIN),
    ("readme", "Markdown"),
];

/// Lowercase extension -> syntax name.
const EXT_OVERRIDES: &[(&str, &str)] = &[
    // JavaScript / TypeScript family
    ("mjs", "JavaScript"),
    ("cjs", "JavaScript"),
    ("jsx", "TypeScriptReact"),
    ("es6", "JavaScript"),
    // JSON variants
    ("jsonc", "JSON"),
    ("json5", "JSON"),
    ("jsonl", "JSON"),
    ("ndjson", "JSON"),
    ("webmanifest", "JSON"),
    ("har", "JSON"),
    ("geojson", "JSON"),
    ("map", "JSON"),
    // HTML-ish templates
    ("cshtml", "HTML"),
    ("razor", "HTML"),
    ("vbhtml", "HTML"),
    ("hbs", "HTML"),
    ("handlebars", "HTML"),
    ("mustache", "HTML"),
    ("ejs", "HTML"),
    ("njk", "HTML"),
    ("liquid", "HTML"),
    ("astro", "HTML"),
    ("xht", "HTML"),
    // Markdown
    ("mdx", "Markdown"),
    ("mkd", "Markdown"),
    ("rmd", "Markdown"),
    // XML family (incl. .NET / MSBuild / Apple / Android)
    ("csproj", "XML"),
    ("fsproj", "XML"),
    ("vbproj", "XML"),
    ("vcxproj", "XML"),
    ("proj", "XML"),
    ("props", "XML"),
    ("targets", "XML"),
    ("nuspec", "XML"),
    ("resx", "XML"),
    ("config", "XML"),
    ("manifest", "XML"),
    ("plist", "XML"),
    ("xib", "XML"),
    ("storyboard", "XML"),
    ("wsdl", "XML"),
    ("xsl", "XML"),
    ("xlf", "XML"),
    ("kml", "XML"),
    ("gpx", "XML"),
    ("axml", "XML"),
    ("uxml", "XML"),
    ("xul", "XML"),
    // Unity
    ("unity", "YAML"),
    ("prefab", "YAML"),
    ("asset", "YAML"),
    ("meta", "YAML"),
    ("mat", "YAML"),
    ("anim", "YAML"),
    ("controller", "YAML"),
    ("asmdef", "JSON"),
    ("asmref", "JSON"),
    ("uss", "CSS"),
    ("shader", "GLSL"),
    ("hlsl", "GLSL"),
    ("cginc", "GLSL"),
    ("compute", "GLSL"),
    ("fx", "GLSL"),
    ("metal", "C++"),
    // C / C++ / misc ambiguity fixes
    ("h", "C++"),
    ("ino", "C++"),
    ("cu", "C++"),
    ("cuh", "C++"),
    ("fs", "F#"),
    ("sln", "INI"),
    ("editorconfig", "INI"),
    ("gitconfig", "Git Config"),
    ("ps1", "Bourne Again Shell (bash)"),
    ("psm1", "Bourne Again Shell (bash)"),
    ("command", "Bourne Again Shell (bash)"),
    ("pyi", "Python"),
    ("gemspec", "Ruby"),
    ("kts", "Kotlin"),
    ("gradle", "Groovy"),
    ("sbt", "Scala"),
    ("vb", "ASP"),
    ("bas", "ASP"),
    ("srt", PLAIN),
    ("text", PLAIN),
];

fn lookup(table: &[(&str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Pick the best syntax name for a file, using its path and contents.
pub fn detect(path: Option<&Path>, text: &str) -> String {
    let ps = &settings().ps;
    if let Some(path) = path {
        let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();

        if let Some(s) = lookup(NAME_OVERRIDES, &name) {
            return s.into();
        }
        // e.g. ".env.local", "Dockerfile", "CMakeLists.txt", "Cargo.lock"
        let raw_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(s) = ps.find_syntax_by_extension(&raw_name) {
            return s.name.clone();
        }
        if name.starts_with(".env") {
            return "DotENV".into();
        }
        if name.starts_with("dockerfile") || name.ends_with(".dockerfile") {
            return "Dockerfile".into();
        }
        if !ext.is_empty() {
            if let Some(s) = lookup(EXT_OVERRIDES, &ext) {
                return s.into();
            }
            if let Some(s) = ps.find_syntax_by_extension(&ext) {
                return s.name.clone();
            }
        }
    }
    // Shebangs, <?xml, <!DOCTYPE html>, etc.
    let first = text.lines().next().unwrap_or("");
    if let Some(s) = ps.find_syntax_by_first_line(first) {
        return s.name.clone();
    }
    PLAIN.into()
}

/// All user-selectable languages, sorted by name.
pub fn all_languages() -> &'static [String] {
    static L: OnceLock<Vec<String>> = OnceLock::new();
    L.get_or_init(|| {
        let mut v: Vec<String> = settings()
            .ps
            .syntaxes()
            .iter()
            .filter(|s| !s.hidden && !s.file_extensions.is_empty())
            .map(|s| s.name.clone())
            .collect();
        v.sort_by_key(|s| s.to_lowercase());
        v.dedup();
        v
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_targets_exist() {
        let ps = &settings().ps;
        for (k, v) in NAME_OVERRIDES.iter().chain(EXT_OVERRIDES) {
            assert!(ps.find_syntax_by_name(v).is_some(), "{k} -> missing syntax {v:?}");
        }
    }

    #[test]
    fn detects_common_files() {
        let cases = [
            ("index.html", "HTML"),
            ("style.css", "CSS"),
            ("app.scss", "SCSS"),
            ("main.js", "JavaScript"),
            ("mod.mjs", "JavaScript"),
            ("App.jsx", "TypeScriptReact"),
            ("App.tsx", "TypeScriptReact"),
            ("lib.ts", "TypeScript"),
            ("README.md", "Markdown"),
            ("Program.cs", "C#"),
            ("Game.csproj", "XML"),
            ("Index.cshtml", "HTML"),
            ("main.rs", "Rust"),
            ("Cargo.toml", "TOML"),
            ("Cargo.lock", "TOML"),
            ("script.py", "Python"),
            ("main.go", "Go"),
            ("Main.java", "Java"),
            ("main.kt", "Kotlin"),
            ("App.swift", "Swift"),
            ("x.h", "C++"),
            ("x.c", "C"),
            ("x.fs", "F#"),
            ("q.sql", "SQL"),
            ("c.yaml", "YAML"),
            ("d.json", "JSON"),
            ("tsconfig.json", "JSON"),
            ("Dockerfile", "Dockerfile"),
            ("Makefile", "Makefile"),
            (".env.local", "DotENV"),
            (".gitignore", "Git Ignore"),
            ("run.sh", "Bourne Again Shell (bash)"),
            ("App.vue", "Vue Component"),
            ("Player.prefab", "YAML"),
            ("notes.txt", PLAIN),
        ];
        for (file, want) in cases {
            assert_eq!(detect(Some(Path::new(file)), ""), want, "{file}");
        }
    }

    #[test]
    fn detects_shebang() {
        assert_eq!(detect(Some(Path::new("tool")), "#!/usr/bin/env python3\n"), "Python");
        assert_eq!(detect(None, "<?xml version=\"1.0\"?>\n"), "XML");
    }
}
