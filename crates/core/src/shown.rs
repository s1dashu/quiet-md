//! What a folder shows, one list for the whole app (docs/FORMAT.md, "What a folder
//! shows"). A file is listed only when Quiet.md opens it: its extension is in
//! `LIBRARY_FILES`, and a `.xml` starts the way DocxXML does and a `.sqlite`
//! holds a 数据表's tables. A scan does not go into folders that hold no notes:
//! version control, dependencies, build output and caches (`skips_dir`), and
//! what a `.gitignore` (at any level) or `.git/info/exclude` leaves out
//! (`Ignore`, the common subset of gitignore). Nothing else about a file is
//! read, listed or written down.

use crate::store::DocKind;
use std::path::Path;

/// The files a folder lists, by extension in any case: their kind, and whether Markdown.
pub const LIBRARY_FILES: [(&str, DocKind, bool); 6] = [
    ("xml", DocKind::Doc, false),
    ("md", DocKind::Doc, true), ("markdown", DocKind::Doc, true), ("mdown", DocKind::Doc, true),
    ("excalidraw", DocKind::Board, false),
    ("sqlite", DocKind::Table, false),
];

/// Attachments a document shows in place: pictures, and a PDF's first page in 预览视图
/// (src/editor/image.rs, src/editor/file.rs). A folder does not list them; they are
/// here so that the day it does, there is still one list.
pub const ATTACHMENTS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "pdf"];

/// A file's kind and whether it is Markdown, by its name (`DocKind::of_name`).
pub fn kind_of_name(name: &str) -> Option<(DocKind, bool)> {
    let ext = name.rsplit_once('.').filter(|(s, _)| !s.is_empty())?.1;
    LIBRARY_FILES.iter().find(|(e, ..)| e.eq_ignore_ascii_case(ext)).map(|&(_, k, md)| (k, md))
}

// ---------------------------------------------------------------- folders not gone into

/// Folders that never hold notes, whatever is beside them, by where they come from.
/// Names starting with `.` are never gone into anyway; they are listed all the same, so
/// that rule can change without losing these.
const SKIP: &[&str] = &[
    // version control
    ".git", ".hg", ".svn",
    // JavaScript / TypeScript: packages, frameworks' build and cache folders, test coverage
    "node_modules", "bower_components", "jspm_packages", ".next", ".nuxt", ".svelte-kit", ".turbo", ".parcel-cache", ".vite", ".angular", ".nyc_output", "storybook-static",
    // C / C++ (CMake); `cmake-build-*` below
    "CMakeFiles",
    // Python; `*.egg-info` below
    "__pycache__", ".venv", "venv", ".tox", ".nox", ".pytest_cache", ".mypy_cache", ".ruff_cache", ".ipynb_checkpoints", ".eggs", "site-packages",
    // JVM, Android, Apple
    ".gradle", ".idea", "DerivedData", "Pods", "Carthage", ".build", "xcuserdata",
    // Dart, Elm, Terraform, Ruby, Sass, Docusaurus, Expo, Serverless, caches
    ".dart_tool", ".pub-cache", "elm-stuff", ".terraform", ".bundle", ".cache", ".sass-cache", ".docusaurus", ".expo", ".serverless",
    // the system's own
    ".Trash", ".Trashes", ".Spotlight-V100", ".fseventsd", "$RECYCLE.BIN", "System Volume Information",
];

/// Names a person may give a folder of notes as well: skipped only beside the
/// project file that makes them build output (`*.x`: any name ending in `.x`).
const BESIDE: &[(&str, &[&str])] = &[
    // Cargo, Maven, sbt
    ("target", &["Cargo.toml", "pom.xml", "build.sbt"]),
    // npm and friends, Gradle, CMake, Python, Flutter, SwiftPM, Xcode, Meson
    ("build", &["package.json", "build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts", "CMakeLists.txt", "setup.py", "pyproject.toml", "pubspec.yaml", "Package.swift", "*.xcodeproj", "meson.build"]),
    ("out", &["package.json", "tsconfig.json", "build.gradle", "build.gradle.kts", "CMakeLists.txt", "*.xcodeproj", "*.iml"]),
    ("dist", &["package.json", "tsconfig.json", "pyproject.toml", "setup.py", "setup.cfg"]),
    // a virtualenv named env
    ("env", &["pyproject.toml", "setup.py", "setup.cfg", "requirements.txt", "Pipfile"]),
    // Go modules, Composer, Bundler (vendor/bundle), cargo vendor
    ("vendor", &["go.mod", "composer.json", "Gemfile", "Cargo.toml"]),
    ("coverage", &["package.json", "pyproject.toml", "setup.py", "Cargo.toml", "go.mod"]),
];

/// Whether a scan leaves folder `name` alone: `beside` are the names next to it,
/// `parent` the name of the folder it is in.
pub fn skips_dir(name: &str, parent: &str, beside: &[String]) -> bool {
    SKIP.contains(&name) || name.starts_with("cmake-build-") || name.ends_with(".egg-info")
        || (name == "Caches" && parent == "Library")
        || BESIDE.iter().any(|(n, marks)| *n == name && beside.iter().any(|b| marks.iter().any(|m| match m.strip_prefix('*') { Some(ext) => b.ends_with(ext) && b.len() > ext.len(), None => b == m })))
}

// ---------------------------------------------------------------- .gitignore

/// One `.gitignore` (or `.git/info/exclude`) and the folder it is in. Supported:
/// `#` comments, `!` to take a path back, a trailing `/` for folders only, a
/// pattern with a `/` in it relative to its folder (else matching a name at any
/// depth), `*`, `?`, `[a-z]` / `[!a-z]`, `**` across folders and `\` escapes.
/// Not supported: the user's global excludes file (`core.excludesFile`).
pub struct Ignore { base: String, rules: Vec<Rule> }
struct Rule { pat: Vec<char>, neg: bool, dir_only: bool, anchored: bool }

impl Ignore {
    /// The rules in `text`, for paths under `base` (relative to the library, "" its top).
    pub fn parse(base: &str, text: &str) -> Self {
        let rules = text.lines().filter_map(|l| {
            let mut l = l.trim_end_matches('\r');
            // Trailing spaces go unless escaped.
            while l.ends_with(' ') && !l.ends_with("\\ ") { l = &l[..l.len() - 1]; }
            if l.is_empty() || l.starts_with('#') { return None }
            let (neg, l) = match l.strip_prefix('!') { Some(r) => (true, r), None => (false, l.strip_prefix('\\').filter(|r| r.starts_with(['#', '!'])).unwrap_or(l)) };
            let (dir_only, l) = match l.strip_suffix('/') { Some(r) => (true, r), None => (false, l) };
            let anchored = l.contains('/');
            let l = l.strip_prefix('/').unwrap_or(l);
            (!l.is_empty()).then(|| Rule { pat: l.chars().collect(), neg, dir_only, anchored })
        }).collect();
        Ignore { base: base.to_string(), rules }
    }

    /// The `.git/info/exclude` and `.gitignore` of folder `rel` (at `dir`), outermost first.
    pub fn read(dir: &Path, rel: &str, has_git: bool, has_ignore: bool) -> Vec<Ignore> {
        let mut out = vec![];
        let mut add = |p: std::path::PathBuf| if let Ok(t) = crate::vfs::read_to_string(p) { out.push(Ignore::parse(rel, &t)) };
        if has_git { add(dir.join(".git").join("info").join("exclude")); }
        if has_ignore { add(dir.join(".gitignore")); }
        out
    }

    /// What this file says about `rel` (relative to the library): `Some(true)` ignored,
    /// `Some(false)` taken back with `!`, `None` nothing.
    fn says(&self, rel: &str, dir: bool) -> Option<bool> {
        let path = if self.base.is_empty() { rel } else { rel.strip_prefix(self.base.as_str())?.strip_prefix('/')? };
        let path: Vec<char> = path.chars().collect();
        let name_at = path.iter().rposition(|c| *c == '/').map_or(0, |i| i + 1);
        self.rules.iter().rev().find(|r| (dir || !r.dir_only) && glob(&r.pat, if r.anchored { &path } else { &path[name_at..] })).map(|r| !r.neg)
    }
}

/// Whether `rel` is ignored by `stack` (the files of its folder and those above,
/// outermost first): a deeper file has the last word, as in git.
pub fn ignored(stack: &[Ignore], rel: &str, dir: bool) -> bool {
    stack.iter().rev().find_map(|i| i.says(rel, dir)).unwrap_or(false)
}

/// gitignore's wildcards: `*` and `?` within a name, `**` across folders.
fn glob(p: &[char], s: &[char]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some('*') if p.get(1) == Some(&'*') => {
            let rest = &p[2..];
            // `**/` is also no folder at all.
            if let Some(after) = rest.strip_prefix(&['/']) && glob(after, s) { return true }
            (0..=s.len()).any(|i| glob(rest, &s[i..]))
        }
        Some('*') => {
            for i in 0..=s.len() {
                if glob(&p[1..], &s[i..]) { return true }
                if s.get(i) == Some(&'/') { break }
            }
            false
        }
        Some('?') => s.first().is_some_and(|c| *c != '/') && glob(&p[1..], &s[1..]),
        Some('[') => {
            let Some(&c) = s.first().filter(|c| **c != '/') else { return false };
            let neg = matches!(p.get(1), Some('!' | '^'));
            let mut i = if neg { 2 } else { 1 };
            let mut hit = false;
            let mut first = true;
            while i < p.len() && (first || p[i] != ']') {
                first = false;
                if p.get(i + 1) == Some(&'-') && p.get(i + 2).is_some_and(|e| *e != ']') { hit |= p[i] <= c && c <= p[i + 2]; i += 3; }
                else { hit |= p[i] == c; i += 1; }
            }
            // No `]`: a plain `[`.
            if i >= p.len() { return c == '[' && glob(&p[1..], &s[1..]) }
            hit != neg && glob(&p[i + 1..], &s[1..])
        }
        Some('\\') if p.len() > 1 => s.first() == Some(&p[1]) && glob(&p[2..], &s[1..]),
        Some(c) => s.first() == Some(c) && glob(&p[1..], &s[1..]),
    }
}

// ---------------------------------------------------------------- files by their contents

/// Whether a file of `kind` at `path` is one Quiet.md opens: a `.xml` starts the way
/// DocxXML does (`docxml_head`, the first KB read), a `.sqlite` holds a 数据表's own
/// tables; the rest by name. A file still only in iCloud is taken by its name (reading
/// it would download it).
pub fn ours(path: &Path, kind: DocKind, markdown: bool) -> bool {
    if markdown || kind == DocKind::Board || crate::vfs::is_dataless(path) { return true }
    match kind {
        DocKind::Doc => {
            use std::io::Read;
            let Ok(mut f) = crate::vfs::File::open(path) else { return false };
            let mut head = [0u8; 1024];
            let mut n = 0;
            while n < head.len() { match f.read(&mut head[n..]) { Ok(0) | Err(_) => break, Ok(k) => n += k } }
            docxml_head(&head[..n])
        }
        #[cfg(not(target_family = "wasm"))]
        DocKind::Table => crate::table::sqlite::is_table_file(path),
        _ => true,
    }
}

/// Whether a `.xml` file starting with `head` is DocxXML: after a byte order mark, an
/// `<?xml …?>` declaration and comments, its first element is `<title>`, a block
/// (`<p>`, `<h1>` …) or a document wrapper. Empty is ours too (a page being written).
/// `pom.xml`'s `<project>`, an Android layout, a plist or HTML's `<!DOCTYPE>` are not.
pub fn docxml_head(head: &[u8]) -> bool {
    let mut s = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head);
    loop {
        let start = s.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(s.len());
        s = &s[start..];
        if s.is_empty() { return true }
        let skip = |s: &[u8], open: &[u8], close: &[u8]| s.strip_prefix(open).and_then(|r| r.windows(close.len()).position(|w| w == close).map(|i| i + open.len() + close.len()));
        if let Some(n) = skip(s, b"<?", b"?>").or_else(|| skip(s, b"<!--", b"-->")) { s = &s[n..]; continue }
        let Some(rest) = s.strip_prefix(b"<") else { return false };
        let end = rest.iter().position(|b| !(b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))).unwrap_or(rest.len());
        let name = std::str::from_utf8(&rest[..end]).unwrap_or("");
        return name == "title" || crate::docxml::is_block_tag(name) || matches!(name, "document" | "doc" | "fragment" | "button");
    }
}

// ---------------------------------------------------------------- before adding a folder

/// What 添加文件夹… asks about before adding a folder (`survey`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Survey {
    /// Nothing to ask.
    Plain,
    /// A code project: a project file or `.git` at its top or in a folder right inside.
    Project,
    /// More than `SURVEY_LIMIT` files and folders.
    Big,
}

/// How many files and folders `survey` counts before calling a folder big.
pub const SURVEY_LIMIT: usize = 20_000;

/// What tells a code project, at its top.
const PROJECT_FILES: &[&str] = &[".git", "package.json", "Cargo.toml", "go.mod", "pyproject.toml", "setup.py", "pom.xml", "build.gradle", "build.gradle.kts", "CMakeLists.txt",
    "Package.swift", "Gemfile", "composer.json", "pubspec.yaml", "*.xcodeproj", "*.sln", "node_modules"];

/// Looks at `dir` before it is added: a project file at its top or one level down,
/// else every file and folder in it counted (all of them, nothing skipped) up to
/// `SURVEY_LIMIT`. Off the main thread: a folder of projects takes a moment.
pub fn survey(dir: &Path) -> Survey {
    let names = |d: &Path| -> Vec<(String, bool)> {
        crate::vfs::read_dir(d).map(|rd| rd.flatten().filter_map(|e| Some((e.file_name().to_str()?.to_string(), e.file_type().ok()?.is_dir()))).collect()).unwrap_or_default()
    };
    let project = |list: &[(String, bool)]| list.iter().any(|(n, _)| PROJECT_FILES.iter().any(|m| match m.strip_prefix('*') { Some(ext) => n.ends_with(ext) && n.len() > ext.len(), None => n == m }));
    let top = names(dir);
    if project(&top) || top.iter().filter(|(n, d)| *d && !n.starts_with('.')).any(|(n, _)| project(&names(&dir.join(n)))) { return Survey::Project }
    let mut count = 0;
    let mut queue = std::collections::VecDeque::from([dir.to_path_buf()]);
    while let Some(d) = queue.pop_front() {
        for (n, is_dir) in names(&d) {
            count += 1;
            if count > SURVEY_LIMIT { return Survey::Big }
            if is_dir { queue.push_back(d.join(n)); }
        }
    }
    Survey::Plain
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }

    #[test]
    fn own_names_are_skipped_anywhere() {
        for n in ["node_modules", "__pycache__", ".git", "Pods", "DerivedData", ".venv", "venv", "site-packages", ".next", "CMakeFiles", "storybook-static", "$RECYCLE.BIN"] {
            assert!(skips_dir(n, "notes", &[]), "{n}");
        }
        assert!(skips_dir("cmake-build-debug", "proj", &[]) && skips_dir("cmake-build-release", "", &[]));
        assert!(skips_dir("mypkg.egg-info", "proj", &[]) && !skips_dir("egg-info notes", "proj", &[]));
        assert!(skips_dir("Caches", "Library", &[]) && !skips_dir("Caches", "Notes", &[]));
        for n in ["Notes", "docs", "src", "项目", "Inbox"] { assert!(!skips_dir(n, "", &names(&["package.json", "Cargo.toml"])), "{n}"); }
    }

    #[test]
    fn common_names_only_beside_their_project_file() {
        for (dir, mark) in [("target", "Cargo.toml"), ("target", "pom.xml"), ("build", "package.json"), ("build", "build.gradle.kts"), ("build", "CMakeLists.txt"), ("out", "tsconfig.json"),
            ("dist", "pyproject.toml"), ("env", "requirements.txt"), ("vendor", "go.mod"), ("vendor", "Gemfile"), ("coverage", "package.json")] {
            assert!(skips_dir(dir, "proj", &names(&["README.md", mark])), "{dir} beside {mark}");
            assert!(!skips_dir(dir, "proj", &names(&["README.md", "notes.md"])), "{dir} alone");
        }
        assert!(skips_dir("build", "App", &names(&["App.xcodeproj"])) && skips_dir("out", "m", &names(&["m.iml"])));
        // A project file of another kind does not count: Cargo's is not Go's.
        assert!(!skips_dir("vendor", "p", &names(&["package.json"])) && !skips_dir("env", "p", &names(&["Cargo.toml"])));
        assert!(!skips_dir("build", "p", &names(&[".xcodeproj"])));
    }

    #[test]
    fn gitignore_rules() {
        let g = [Ignore::parse("", "# build output\n/dist\n*.log\ntmp/\n!keep.log\ndocs/**/draft-*.md\n\\#hash\n**/gen\nlogs/*.md\n")];
        let ig = |p: &str, dir: bool| ignored(&g, p, dir);
        assert!(ig("dist", true) && !ig("sub/dist", true), "anchored to its folder");
        assert!(ig("a.log", false) && ig("deep/b.log", false) && !ig("keep.log", false) && !ig("deep/keep.log", false));
        assert!(ig("tmp", true) && ig("x/tmp", true) && !ig("tmp", false), "folders only");
        assert!(ig("docs/draft-1.md", false) && ig("docs/a/b/draft-2.md", false) && !ig("docs/final.md", false));
        assert!(ig("#hash", false) && ig("gen", true) && ig("a/b/gen", true));
        assert!(ig("logs/today.md", false) && !ig("logs/old/today.md", false), "* stays within a folder");
        let g = [Ignore::parse("", "[abc]?.md\n[!x]y.md\n")];
        assert!(ignored(&g, "a1.md", false) && !ignored(&g, "d1.md", false) && ignored(&g, "zy.md", false) && !ignored(&g, "xy.md", false));
    }

    #[test]
    fn deeper_gitignores_have_the_last_word() {
        let stack = [Ignore::parse("", "*.md\n"), Ignore::parse("notes", "!*.md\nprivate.md\n")];
        assert!(ignored(&stack, "README.md", false) && ignored(&stack, "src/a.md", false));
        assert!(!ignored(&stack, "notes/a.md", false) && !ignored(&stack, "notes/sub/b.md", false));
        assert!(ignored(&stack, "notes/private.md", false));
        // `.git/info/exclude` comes before the folder's `.gitignore`.
        let stack = [Ignore::parse("", "secret/\n"), Ignore::parse("", "!secret/\n")];
        assert!(!ignored(&stack, "secret", true));
    }

    #[test]
    fn xml_files_by_their_first_element() {
        for ok in ["<title>周报</title>\n<p>x</p>", "\u{feff}<title icon=\"📘\">a</title>", "<?xml version=\"1.0\"?>\n<!-- made by hand -->\n<p>x</p>", "<h1>a</h1>", "<document><title>a</title></document>", "", "  \n"] {
            assert!(docxml_head(ok.as_bytes()), "{ok:?}");
        }
        for not in ["<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project xmlns=\"http://maven.apache.org/POM/4.0.0\">", "<LinearLayout android:id=\"@+id/x\">", "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\">\n<plist>", "<manifest>", "plain text", "<?xml version=\"1.0\""] {
            assert!(!docxml_head(not.as_bytes()), "{not:?}");
        }
    }

    #[test]
    fn a_folder_is_looked_at_before_it_is_added() {
        let d = crate::vfs::temp_dir().join(format!("fdl-survey-{}", uuid::Uuid::new_v4()));
        let w = |rel: &str| { let p = d.join(rel); crate::vfs::create_dir_all(p.parent().unwrap()).unwrap(); crate::vfs::write(p, "x").unwrap(); };
        w("notes/a.md");
        w("notes/b/c.md");
        assert_eq!(survey(&d.join("notes")), Survey::Plain);
        w("repos/one/Cargo.toml");
        assert_eq!(survey(&d.join("repos")), Survey::Project, "a folder of projects");
        assert_eq!(survey(&d.join("repos/one")), Survey::Project);
        w("app/App.xcodeproj/project.pbxproj");
        assert_eq!(survey(&d.join("app")), Survey::Project);
        for i in 0..SURVEY_LIMIT / 1000 + 1 { for j in 0..1000 { w(&format!("big/{i}/{j}.txt")); } }
        assert_eq!(survey(&d.join("big")), Survey::Big);
        let _ = crate::vfs::remove_dir_all(&d);
    }

    #[test]
    fn the_one_list_of_extensions() {
        assert_eq!(kind_of_name("周报.XML"), Some((DocKind::Doc, false)));
        assert_eq!(kind_of_name("a.Markdown"), Some((DocKind::Doc, true)));
        assert_eq!(kind_of_name("b.excalidraw"), Some((DocKind::Board, false)));
        assert_eq!(kind_of_name("c.sqlite"), Some((DocKind::Table, false)));
        for n in ["main.rs", ".md", "x.txt", "pic.png", "noext"] { assert_eq!(kind_of_name(n), None, "{n}"); }
    }
}
