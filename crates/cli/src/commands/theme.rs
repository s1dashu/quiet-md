//! `qmd theme` — inspect, select, clone, modify and lint Quiet.md CSS themes.

use super::read_arg_or_stdin;
use crate::args::ParsedCmd;
use crate::output::{CliFailure, Diagnostic, GlobalOpts, print_success};
use fdl_core::store;
use fdl_core::theme_css::{self, BUNDLED, CssVal, PROPS, TOKEN_NAMES};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn themes_dir(root: &Path) -> PathBuf {
    root.join(store::INTERNAL).join("themes")
}

fn check_theme_id(id: &str) -> Result<&str, CliFailure> {
    let t = id.trim();
    if t.is_empty() || t.contains('/') || t.contains('\\') || t.contains("..") || t.contains('\0') {
        return Err(CliFailure::new("INVALID_THEME_ID", format!("Invalid theme id '{id}'.")));
    }
    Ok(t)
}

fn read_theme_css(root: &Path, id: &str) -> Result<(String, bool), CliFailure> {
    let custom = themes_dir(root).join(format!("{id}.css"));
    if custom.is_file() {
        let css = std::fs::read_to_string(&custom)
            .map_err(|e| CliFailure::new("THEME_READ_ERROR", format!("Failed reading {}: {e}", custom.display())))?;
        return Ok((css, true));
    }
    let (_, css) = BUNDLED.iter().find(|(b, _)| *b == id).ok_or_else(|| {
        let mut valid: Vec<String> = BUNDLED.iter().map(|(b, _)| (*b).to_string()).collect();
        if let Ok(rd) = std::fs::read_dir(themes_dir(root)) {
            for e in rd.flatten() {
                if let Some(stem) = e.path().file_stem().and_then(|s| s.to_str()) && e.path().extension().is_some_and(|x| x == "css") {
                    valid.push(stem.to_string());
                }
            }
        }
        CliFailure::new("THEME_NOT_FOUND", format!("Unknown theme '{id}'.")).with_valid(valid)
    })?;
    Ok(((*css).to_string(), false))
}

fn list_all_theme_ids(root: &Path) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for (id, _) in BUNDLED {
        ids.insert((*id).to_string());
    }
    if let Ok(rd) = std::fs::read_dir(themes_dir(root)) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() && p.extension().is_some_and(|x| x == "css")
                && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
            {
                ids.insert(stem.to_string());
            }
        }
    }
    ids.into_iter().collect()
}

/// Update or insert a CSS variable declaration inside `:root { ... }`.
fn upsert_root_var(css: &str, var: &str, val: &str) -> String {
    if let Some(root_pos) = css.find(":root")
        && let Some(rel_open) = css[root_pos..].find('{')
    {
        let open = root_pos + rel_open;
        if let Some(rel_close) = css[open + 1..].find('}') {
            let close = open + 1 + rel_close;
            let body = &css[open + 1..close];
            let mut out = String::with_capacity(body.len() + var.len() + val.len() + 8);
            let mut replaced = false;
            for line in body.split_inclusive('\n') {
                let trimmed = line.trim_start();
                let is_comment = trimmed.starts_with("/*");
                if !is_comment && let Some(rest) = trimmed.strip_prefix(var) && rest.trim_start().starts_with(':') {
                    let indent_len = line.len() - trimmed.len();
                    let indent = &line[..indent_len];
                    let nl = if line.ends_with('\n') { "\n" } else { "" };
                    out.push_str(&format!("{indent}{var}: {val};{nl}"));
                    replaced = true;
                } else {
                    out.push_str(line);
                }
            }
            if !replaced {
                if !out.ends_with('\n') && !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&format!("  {var}: {val};\n"));
            }
            let mut result = String::with_capacity(css.len() + var.len() + val.len() + 16);
            result.push_str(&css[..open + 1]);
            result.push_str(&out);
            result.push_str(&css[close..]);
            return result;
        }
    }
    format!(":root {{\n  {var}: {val};\n}}\n\n{css}")
}

fn css_val_json(v: &CssVal) -> serde_json::Value {
    match v {
        CssVal::Len(n) | CssVal::Weight(n) => serde_json::json!(n),
        CssVal::Color(c) => serde_json::json!(theme_css::rgba_css(*c)),
        CssVal::Inherit => serde_json::json!("inherit"),
        CssVal::Font(f) => serde_json::json!(f),
        CssVal::Bool(b) => serde_json::json!(b),
        CssVal::Line(s) => serde_json::json!(s),
    }
}

pub fn run(cmd: &ParsedCmd, opts: &GlobalOpts) -> Result<(), CliFailure> {
    let root = super::resolve_library_root(opts);
    match cmd.command_path.as_str() {
        "theme list" => {
            let active = theme_css::active_theme_id(&root);
            let mut list = vec![];
            for id in list_all_theme_ids(&root) {
                let bundled = BUNDLED.iter().any(|(b, _)| *b == id);
                let custom = themes_dir(&root).join(format!("{id}.css")).is_file();
                let parsed = theme_css::load_theme_for_library(&root, &id).ok();
                list.push(serde_json::json!({
                    "id": id,
                    "active": id == active,
                    "bundled": bundled,
                    "custom": custom,
                    "base": parsed.as_ref().map(|p| if p.feishu { "feishu" } else { "craft" }),
                    "name": parsed.as_ref().and_then(|p| p.meta.name.clone()),
                    "name_en": parsed.as_ref().and_then(|p| p.meta.name_en.clone()),
                    "author": parsed.as_ref().and_then(|p| p.meta.author.clone()),
                    "warnings_count": parsed.as_ref().map(|p| p.warnings.len()).unwrap_or(0),
                }));
            }
            let human = list.iter().map(|t| {
                let mark = if t["active"].as_bool() == Some(true) { "*" } else { " " };
                let id = t["id"].as_str().unwrap_or("");
                let name = t["name"].as_str().or(t["name_en"].as_str()).unwrap_or(id);
                let base = t["base"].as_str().unwrap_or("?");
                format!("{mark} {id:<14} {name}  (base: {base})")
            }).collect::<Vec<_>>().join("\n");
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "active": active, "themes": list }),
                &[], &[], &human,
            );
            Ok(())
        }
        "theme show" => {
            let id = check_theme_id(cmd.pos(0, "<theme-id>")?)?;
            let raw = cmd.bool_flag("raw")?.unwrap_or(false);
            let (css, custom) = read_theme_css(&root, id)?;
            let parsed = theme_css::parse_css(id, &css).map_err(|e| CliFailure::new("THEME_PARSE_ERROR", e))?;
            let props: serde_json::Map<String, serde_json::Value> = parsed.props.iter().map(|(k, v)| ((*k).to_string(), css_val_json(v))).collect();
            let palette: serde_json::Map<String, serde_json::Value> = parsed.palette.iter().map(|(k, c)| (k.clone(), serde_json::json!(theme_css::rgba_css(*c)))).collect();
            let tokens: serde_json::Map<String, serde_json::Value> = parsed.tokens.iter().map(|(k, c)| (k.clone(), serde_json::json!(theme_css::rgba_css(*c)))).collect();
            let data = serde_json::json!({
                "id": id,
                "custom": custom,
                "base": if parsed.feishu { "feishu" } else { "craft" },
                "meta": {
                    "name": parsed.meta.name,
                    "name_en": parsed.meta.name_en,
                    "author": parsed.meta.author,
                    "description": parsed.meta.description,
                },
                "vars": parsed.vars,
                "props": props,
                "palette": palette,
                "tokens": tokens,
                "warnings": parsed.warnings,
                "css": if raw { Some(css) } else { None },
            });
            print_success(opts, &cmd.command_path, Some(&root), data.clone(), &[], &[], &serde_json::to_string_pretty(&data).unwrap_or_default());
            Ok(())
        }
        "theme props" => {
            let selectors: Vec<_> = PROPS.iter().map(|p| serde_json::json!({
                "selector": p.sel,
                "property": p.prop,
                "key": p.key,
                "kind": format!("{:?}", p.kind),
                "feishu_used": p.feishu_used,
            })).collect();
            let token_vars: Vec<String> = TOKEN_NAMES.iter().map(|t| format!("--token-{}", t.replace('_', "-"))).collect();
            let data = serde_json::json!({
                "selectors": selectors,
                "token_vars": token_vars,
                "palette_var_prefixes": ["--color-<hue>", "--background-<name>", "--callout-<name>", "--highlight-<name>", "--border-<hue>"],
                "meta_vars": ["--theme-base", "--theme-name", "--theme-name-en", "--theme-author", "--theme-description"],
            });
            print_success(
                opts, &cmd.command_path, Some(&root),
                data.clone(), &[], &[],
                &format!("{} CSS selector/property pairs, {} UI tokens.", selectors.len(), token_vars.len()),
            );
            Ok(())
        }
        "theme select" => {
            let id = check_theme_id(cmd.pos(0, "<theme-id>")?)?;
            let parsed = theme_css::load_theme_for_library(&root, id).map_err(|e| CliFailure::new("THEME_NOT_FOUND", e))?;
            let pref_path = root.join(store::INTERNAL).join("preferences.json");
            let mut obj = if pref_path.is_file() {
                let raw = std::fs::read_to_string(&pref_path).map_err(|e| CliFailure::new("PREFS_READ_ERROR", e.to_string()))?;
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&raw).unwrap_or_default()
            } else {
                serde_json::Map::new()
            };
            obj.insert("theme".into(), serde_json::json!(id));
            obj.insert("feishu".into(), serde_json::json!(parsed.feishu));
            if !opts.dry_run {
                if let Some(p) = pref_path.parent() {
                    std::fs::create_dir_all(p).map_err(|e| CliFailure::new("PREFS_WRITE_ERROR", e.to_string()))?;
                }
                let json = serde_json::to_string_pretty(&serde_json::Value::Object(obj)).unwrap();
                std::fs::write(&pref_path, json).map_err(|e| CliFailure::new("PREFS_WRITE_ERROR", e.to_string()))?;
            }
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "active": id, "base": if parsed.feishu { "feishu" } else { "craft" } }),
                &[], &[],
                &format!("Selected theme '{id}'"),
            );
            Ok(())
        }
        "theme create" => {
            let new_id = check_theme_id(cmd.pos(0, "<new-id>")?)?;
            let dest = themes_dir(&root).join(format!("{new_id}.css"));
            if dest.exists() {
                return Err(CliFailure::new("THEME_EXISTS", format!("Custom theme '{new_id}' already exists at {}.", dest.display()))
                    .with_hint("Use `qmd theme set-var` to edit it or `qmd theme reset` to remove it first."));
            }
            let mut css = if let Some(css_arg) = cmd.flag("css") {
                read_arg_or_stdin(css_arg)?
            } else {
                let from = check_theme_id(cmd.flag("from").unwrap_or("craft"))?;
                read_theme_css(&root, from)?.0
            };
            if let Some(name) = cmd.flag("name") {
                let quoted = format!("\"{}\"", name.replace('"', "\\\""));
                css = upsert_root_var(&css, "--theme-name", &quoted);
                css = upsert_root_var(&css, "--theme-name-en", &quoted);
            }
            let parsed = theme_css::parse_css(new_id, &css).map_err(|e| CliFailure::new("INVALID_THEME_CSS", e))?;
            let diags: Vec<Diagnostic> = parsed.warnings.iter().map(|w| Diagnostic {
                severity: "warning", rule: "theme-css".into(), loc: None, message: w.clone(), fix_hint: None,
            }).collect();
            if !opts.dry_run {
                std::fs::create_dir_all(themes_dir(&root)).map_err(|e| CliFailure::new("THEME_WRITE_ERROR", e.to_string()))?;
                std::fs::write(&dest, &css).map_err(|e| CliFailure::new("THEME_WRITE_ERROR", e.to_string()))?;
            }
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": new_id, "path": dest.display().to_string(), "base": if parsed.feishu { "feishu" } else { "craft" } }),
                &[], &diags,
                &format!("Created theme '{new_id}' at {}", dest.display()),
            );
            Ok(())
        }
        "theme set-var" => {
            let id = check_theme_id(cmd.pos(0, "<theme-id>")?)?;
            let var = cmd.pos(1, "<var-name>")?;
            let val = cmd.pos(2, "<value>")?;
            if !var.starts_with("--") || !var[2..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                return Err(CliFailure::usage(format!("CSS variable '{var}' must start with `--` and use ASCII letters/digits/`-`/`_`.")));
            }
            let (css, _) = read_theme_css(&root, id)?;
            let updated = upsert_root_var(&css, var, val);
            let parsed = theme_css::parse_css(id, &updated).map_err(|e| CliFailure::new("INVALID_THEME_CSS", e))?;
            let dest = themes_dir(&root).join(format!("{id}.css"));
            if !opts.dry_run {
                std::fs::create_dir_all(themes_dir(&root)).map_err(|e| CliFailure::new("THEME_WRITE_ERROR", e.to_string()))?;
                std::fs::write(&dest, &updated).map_err(|e| CliFailure::new("THEME_WRITE_ERROR", e.to_string()))?;
            }
            let diags: Vec<Diagnostic> = parsed.warnings.iter().map(|w| Diagnostic {
                severity: "warning", rule: "theme-css".into(), loc: None, message: w.clone(), fix_hint: None,
            }).collect();
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": id, "var": var, "value": val, "path": dest.display().to_string() }),
                &[], &diags,
                &format!("Updated {var}: {val} in theme '{id}'"),
            );
            Ok(())
        }
        "theme lint" => {
            let target = cmd.pos(0, "<theme-id|file>")?;
            let p = Path::new(target);
            let (id, css) = if p.is_file() {
                (
                    p.file_stem().and_then(|s| s.to_str()).unwrap_or("custom").to_string(),
                    std::fs::read_to_string(p).map_err(|e| CliFailure::new("THEME_READ_ERROR", e.to_string()))?,
                )
            } else {
                let id = check_theme_id(target)?;
                (id.to_string(), read_theme_css(&root, id)?.0)
            };
            let parsed = theme_css::parse_css(&id, &css).map_err(|e| CliFailure::new("INVALID_THEME_CSS", e))?;
            let diags: Vec<Diagnostic> = parsed.warnings.iter().map(|w| Diagnostic {
                severity: "warning", rule: "theme-css-unused-or-invalid".into(), loc: None, message: w.clone(),
                fix_hint: Some("Run `qmd theme props --json` for valid selectors, properties and tokens.".into()),
            }).collect();
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({
                    "id": id,
                    "base": if parsed.feishu { "feishu" } else { "craft" },
                    "props_count": parsed.props.len(),
                    "tokens_count": parsed.tokens.len(),
                    "palette_count": parsed.palette.len(),
                    "warnings_count": parsed.warnings.len(),
                }),
                &[], &diags,
                &format!("Linted theme '{id}': {} prop(s), {} token(s), {} warning(s)", parsed.props.len(), parsed.tokens.len(), parsed.warnings.len()),
            );
            Ok(())
        }
        "theme reset" => {
            let id = check_theme_id(cmd.pos(0, "<theme-id>")?)?;
            let dest = themes_dir(&root).join(format!("{id}.css"));
            if !dest.is_file() {
                return Err(CliFailure::new("NOT_CUSTOM_THEME", format!("No custom theme file exists at {}.", dest.display())));
            }
            if !opts.dry_run {
                std::fs::remove_file(&dest).map_err(|e| CliFailure::new("THEME_DELETE_ERROR", e.to_string()))?;
                if !BUNDLED.iter().any(|(b, _)| *b == id) && theme_css::active_theme_id(&root) == id {
                    let pref_path = root.join(store::INTERNAL).join("preferences.json");
                    if let Ok(raw) = std::fs::read_to_string(&pref_path)
                        && let Ok(mut obj) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&raw)
                    {
                        obj.insert("theme".into(), serde_json::json!("feishu"));
                        obj.insert("feishu".into(), serde_json::json!(true));
                        let _ = std::fs::write(&pref_path, serde_json::to_string_pretty(&serde_json::Value::Object(obj)).unwrap());
                    }
                }
            }
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": id, "removed": true }),
                &[], &[],
                &format!("Removed custom theme file for '{id}'"),
            );
            Ok(())
        }
        _ => Err(CliFailure::usage(format!("Unhandled theme command {}", cmd.command_path))),
    }
}
