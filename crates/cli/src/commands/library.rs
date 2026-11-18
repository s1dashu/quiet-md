//! `qmd library` — init, status, tree, search.

use super::{open_store, resolve_library_root};
use crate::args::ParsedCmd;
use crate::output::{CliFailure, GlobalOpts, print_success};
use fdl_core::store::{Entry, PageWidth, Store};
use fdl_core::theme_css;
use serde::Serialize;

#[derive(Serialize)]
struct EntryNode {
    id: String,
    kind: &'static str, // "folder" | "doc" | "markdown" | "board"
    title: String,
    rel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    pinned: bool,
    starred: bool,
    width: &'static str,
    created: u64,
    modified: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<EntryNode>,
}

fn kind_of(e: &Entry) -> &'static str {
    if e.meta.folder.is_some() { "folder" }
    else if !e.meta.kind.is_doc() { "board" }
    else if e.meta.markdown { "markdown" }
    else { "doc" }
}

fn width_str(w: PageWidth) -> &'static str {
    match w { PageWidth::Standard => "standard", PageWidth::Wide => "wide", PageWidth::Full => "full" }
}

fn to_node(e: &Entry, all: &[Entry], nested: bool) -> EntryNode {
    let mut children = vec![];
    if nested && e.meta.folder.is_some() {
        let mut kids: Vec<&Entry> = all.iter().filter(|c| c.meta.parent.as_deref() == Some(&e.id)).collect();
        kids.sort_by(|a, b| (a.meta.place(), &a.id).cmp(&(b.meta.place(), &b.id)));
        children = kids.into_iter().map(|k| to_node(k, all, true)).collect();
    }
    EntryNode {
        id: e.id.clone(),
        kind: kind_of(e),
        title: e.title.clone(),
        rel: e.meta.rel.clone(),
        icon: e.icon.clone(),
        pinned: e.meta.pinned,
        starred: e.meta.starred > 0,
        width: width_str(e.meta.width),
        created: e.meta.created,
        modified: e.meta.modified,
        parent: e.meta.parent.clone(),
        children,
    }
}

pub fn run(cmd: &ParsedCmd, opts: &GlobalOpts) -> Result<(), CliFailure> {
    match cmd.command_path.as_str() {
        "library init" => {
            let root = resolve_library_root(opts);
            if !opts.dry_run {
                let _ = Store::open_at(root.clone())?;
            }
            let data = serde_json::json!({ "root": root.display().to_string(), "format": fdl_core::store::FORMAT });
            print_success(opts, &cmd.command_path, Some(&root), data, &[], &[], &format!("Initialized library at {}", root.display()));
            Ok(())
        }
        "library status" => {
            let mut store = open_store(opts)?;
            let entries = store.list();
            let folders = entries.iter().filter(|e| e.meta.folder.is_some()).count();
            let docs = entries.len() - folders;
            let theme = theme_css::active_theme_id(store.root());
            let data = serde_json::json!({
                "root": store.root().display().to_string(),
                "format": fdl_core::store::FORMAT,
                "folders": folders,
                "documents": docs,
                "active_theme": theme,
            });
            let human = format!("Library: {}\nFormat: {}\nFolders: {folders}, Documents: {docs}\nTheme: {theme}", store.root().display(), fdl_core::store::FORMAT);
            print_success(opts, &cmd.command_path, Some(store.root()), data, &store.warnings, &[], &human);
            Ok(())
        }
        "library tree" => {
            let mut store = open_store(opts)?;
            let mut entries = store.list();
            entries.sort_by(|a, b| (a.meta.place(), &a.id).cmp(&(b.meta.place(), &b.id)));
            let flat = cmd.has_flag("flat");
            let nodes: Vec<EntryNode> = if flat {
                entries.iter().map(|e| to_node(e, &entries, false)).collect()
            } else {
                entries.iter().filter(|e| e.meta.parent.is_none()).map(|e| to_node(e, &entries, true)).collect()
            };
            let mut human = String::new();
            if !opts.json {
                fn fmt_tree(nodes: &[EntryNode], depth: usize, out: &mut String) {
                    use std::fmt::Write;
                    for n in nodes {
                        let indent = "  ".repeat(depth);
                        let ic = n.icon.as_deref().unwrap_or(if n.kind == "folder" { "📁" } else { "📄" });
                        let pin = if n.pinned { " [pinned]" } else { "" };
                        let _ = writeln!(out, "{indent}{ic} {} ({}) [{}]{pin}", n.title, n.rel, n.id);
                        fmt_tree(&n.children, depth + 1, out);
                    }
                }
                fmt_tree(&nodes, 0, &mut human);
            }
            print_success(opts, &cmd.command_path, Some(store.root()), nodes, &store.warnings, &[], human.trim_end());
            Ok(())
        }
        "library search" => {
            let q = cmd.pos(0, "<query>")?.to_lowercase();
            let limit: usize = cmd.flag("limit").and_then(|s| s.parse().ok()).unwrap_or(20);
            let mut store = open_store(opts)?;
            let entries = store.list();
            let mut hits = vec![];
            for e in entries {
                if e.meta.folder.is_some() { continue; }
                let t_hit = e.title.to_lowercase().contains(&q);
                let b_hit = e.body.to_lowercase().contains(&q);
                if t_hit || b_hit {
                    let snippet: String = e.body.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect();
                    hits.push(serde_json::json!({
                        "id": e.id,
                        "title": e.title,
                        "rel": e.meta.rel,
                        "title_match": t_hit,
                        "snippet": snippet,
                    }));
                }
                if hits.len() >= limit { break; }
            }
            let human = format!("Found {} matching document(s).", hits.len());
            print_success(opts, &cmd.command_path, Some(store.root()), hits, &store.warnings, &[], &human);
            Ok(())
        }
        _ => Err(CliFailure::usage(format!("Unhandled library command {}", cmd.command_path))),
    }
}
