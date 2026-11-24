//! Finding an agent's executable (`codex`, `gemini`, `claude`, …). An app
//! started from Finder or the Start menu doesn't get the shell's PATH, so the
//! usual install places are tried before it (docs/design/ai-assistant.md ›
//! 找到 codex). Nothing is bundled: no Node, no agent.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The executable `name`: the path set in Settings when there is one, then
/// the install places (Homebrew, `~/.local/bin`, npm global, Volta, mise and
/// asdf shims), then PATH. On Windows `name.exe` or `name.cmd` (npm's shim).
/// None: not installed.
pub fn program(configured: Option<&Path>, name: &str) -> Option<PathBuf> { program_in(configured, name, &|k| std::env::var_os(k)) }

/// `program` with the environment read through `env` (for tests).
pub fn program_in(configured: Option<&Path>, name: &str, env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let names = file_names(name);
    if let Some(p) = configured.filter(|p| !p.as_os_str().is_empty()) {
        // A folder holding it, or the executable itself.
        return if p.is_dir() { in_dir(p, &names) } else { p.is_file().then(|| p.to_path_buf()) };
    }
    dirs(env).iter().find_map(|d| in_dir(d, &names))
}

fn file_names(name: &str) -> Vec<String> {
    if cfg!(windows) { vec![format!("{name}.exe"), format!("{name}.cmd")] } else { vec![name.to_string()] }
}

/// The folders tried, in order.
fn dirs(env: &dyn Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let var = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let mut out = vec![];
    if cfg!(windows) {
        if let Some(a) = var("APPDATA") { out.push(a.join("npm")); }
        if let Some(p) = var("NPM_CONFIG_PREFIX") { out.push(p); }
        if let Some(l) = var("LOCALAPPDATA") { out.extend([l.join("Volta").join("bin"), l.join("mise").join("shims")]); }
    } else {
        out.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
        let home = var("HOME");
        if let Some(h) = &home { out.push(h.join(".local").join("bin")); }
        if let Some(p) = var("NPM_CONFIG_PREFIX") { out.push(p.join("bin")); }
        if let Some(h) = &home { out.extend([h.join(".npm-global").join("bin"), h.join(".volta").join("bin")]); }
        out.push(var("MISE_DATA_DIR").or_else(|| home.as_ref().map(|h| h.join(".local").join("share").join("mise"))).unwrap_or_default().join("shims"));
        out.push(var("ASDF_DATA_DIR").or_else(|| home.as_ref().map(|h| h.join(".asdf"))).unwrap_or_default().join("shims"));
    }
    if let Some(path) = env("PATH") { out.extend(std::env::split_paths(&path)); }
    out.retain(|d| d.is_absolute());
    out
}

fn in_dir(dir: &Path, names: &[String]) -> Option<PathBuf> { names.iter().map(|n| dir.join(n)).find(|p| p.is_file()) }

#[cfg(test)]
mod tests {
    use super::*;

    /// Settings' path wins; then the install places; PATH comes last.
    #[test]
    fn order() {
        let d = std::env::temp_dir().join(format!("quiet-agent-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let name = "quiet-agent-test-tool";
        let file = &file_names(name)[0];
        let (set, path, local) = (d.join("set"), d.join("path"), d.join("home").join(if cfg!(windows) { "npm" } else { ".local/bin" }));
        for f in [&set, &path, &local] { std::fs::create_dir_all(f).unwrap(); std::fs::write(f.join(file), "").unwrap(); }
        let env = |k: &str| match k {
            "PATH" => Some(path.clone().into_os_string()),
            "HOME" | "APPDATA" => Some(d.join("home").into_os_string()),
            _ => None,
        };
        assert_eq!(program_in(Some(&set), name, &env), Some(set.join(file)));
        assert_eq!(program_in(Some(&set.join(file)), name, &env), Some(set.join(file)));
        assert_eq!(program_in(None, name, &env), Some(local.join(file)));
        std::fs::remove_file(local.join(file)).unwrap();
        assert_eq!(program_in(None, name, &env), Some(path.join(file)));
        // A configured path that isn't there is not looked past.
        assert_eq!(program_in(Some(&d.join("gone")), name, &env), None);
        assert_eq!(program_in(None, "quiet-agent-no-such-tool", &env), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
