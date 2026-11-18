use quiet_cli::board_builder::render_svg_to_png;
use quiet_cli::{run_args, should_intercept_args};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("qmd-e2e-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn qmd_bin(args: &[&str]) -> (i32, Value, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_qmd")).args(args).output().expect("run qmd bin");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let v = serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("expected JSON stdout for {args:?}: {e}\nstdout={stdout}\nstderr={stderr}"));
    (code, v, stderr)
}

#[test]
fn forwarding_does_not_intercept_gui_args() {
    let s = |xs: &[&str]| xs.iter().map(|x| (*x).to_string()).collect::<Vec<_>>();
    assert!(!should_intercept_args(&s(&["quiet_md"])));
    assert!(!should_intercept_args(&s(&["quiet_md", "notes/readme.md"])));
    assert!(!should_intercept_args(&s(&["quiet_md", "readme.md"])));
    assert!(!should_intercept_args(&s(&["quiet_md", "-psn_0_12345"])));
    assert!(should_intercept_args(&s(&["quiet_md", "--help"])));
    assert!(should_intercept_args(&s(&["quiet_md", "--json", "schema", "palette"])));
    assert!(should_intercept_args(&s(&["quiet_md", "--lib", "/tmp/a", "library", "status"])));
    assert!(should_intercept_args(&s(&["quiet_md", "doc", "create"])));
    assert!(should_intercept_args(&s(&["quiet_md", "cli", "schema"])));
}

#[test]
fn schema_commands_and_palette() {
    let (code, v, _) = qmd_bin(&["--json", "schema", "all"]);
    assert_eq!(code, 0);
    assert_eq!(v["ok"], true);
    let covers = v["data"]["covers"]["all"].as_array().unwrap();
    assert_eq!(covers.len(), 26);
    assert_eq!(v["data"]["palette"]["font_colors"].as_array().unwrap().len(), 7);
    assert!(!v["data"]["commands"].as_array().unwrap().is_empty());

    let (c2, v2, _) = qmd_bin(&["--json", "schema", "themes"]);
    assert_eq!(c2, 0);
    assert_eq!(v2["data"]["bundled_themes"].as_array().unwrap().len(), 4);
}

#[test]
fn end_to_end_document_board_table_theme_validate_and_export() {
    let tmp = TempDir::new("full");
    let lib = tmp.path().join("lib");
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let lib_s = lib.to_str().unwrap();

    // 1. Initialize library & folder tree
    let (c, v, _) = qmd_bin(&["--lib", lib_s, "--json", "library", "init"]);
    assert_eq!(c, 0, "{v}");
    let (c, f1, _) = qmd_bin(&["--lib", lib_s, "--json", "folder", "create", "Architecture"]);
    assert_eq!(c, 0, "{f1}");
    let (c, f2, _) = qmd_bin(&["--lib", lib_s, "--json", "folder", "create", "RFCs", "--parent", "Architecture"]);
    assert_eq!(c, 0, "{f2}");
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "folder", "rename", "RFCs", "Design RFCs"]);
    assert_eq!(c, 0);

    // 2. Import an image asset
    let img_file = work.join("badge.png");
    let png_bytes = render_svg_to_png(
        "<svg xmlns='http://www.w3.org/2000/svg' width='120' height='60'><rect width='120' height='60' rx='8' fill='#2563eb'/></svg>",
        1.0,
    ).unwrap();
    std::fs::write(&img_file, &png_bytes).unwrap();
    let (c, img_res, _) = qmd_bin(&["--lib", lib_s, "--json", "asset", "add-image", img_file.to_str().unwrap()]);
    assert_eq!(c, 0, "{img_res}");
    assert_eq!(img_res["data"]["width"], 120);
    assert_eq!(img_res["data"]["height"], 60);

    // 3. Create target doc for @doc link testing
    let (c, ref_doc, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "create", "--folder", "Architecture", "--title", "Glossary"]);
    assert_eq!(c, 0, "{ref_doc}");
    let ref_id = ref_doc["data"]["id"].as_str().unwrap().to_string();

    // 4. Create rich aesthetic document declaratively via DocSpec + --strict
    let spec = json!({
        "title": "System Blueprint 2026",
        "icon": "rocket",
        "cover": { "gallery": "aurora-2", "y": 0.2 },
        "meta": { "width": "wide", "catalogue_open": true, "pinned": true, "starred": true },
        "blocks": [
            {
                "type": "callout",
                "emoji": "bulb",
                "bg": "light-blue",
                "border": "blue",
                "children": [
                    {
                        "type": "p",
                        "runs": [
                            { "text": "Summary: ", "bold": true },
                            { "text": "End-to-end p99 latency stays under " },
                            { "text": "18ms", "bold": true, "text_color": "blue", "bg_color": "light-blue" },
                            { "text": ". See " },
                            { "doc_link": { "target": ref_id } },
                            { "text": " and " },
                            { "equation": "E = mc^2" }
                        ]
                    }
                ]
            },
            { "type": "h1", "seq_auto": true, "text": "Architecture Overview" },
            {
                "type": "grid",
                "columns": [
                    { "ratio": 0.6, "children": [{ "type": "p", "text": "Primary write path uses zero-copy ring buffers." }] },
                    { "ratio": 0.4, "children": [{ "type": "todo", "done": false, "text": "Enable canary shard rollout" }] }
                ]
            },
            {
                "type": "board",
                "height": 340,
                "diagram": {
                    "layout": "flow-lr",
                    "theme": "craft-pastel",
                    "gap_x": 100,
                    "gap_y": 56,
                    "nodes": [
                        { "id": "api", "shape": "rectangle", "label": "API Edge\ngRPC", "rounded": true },
                        { "id": "core", "shape": "rectangle", "label": "Core Engine\nRust", "rounded": true },
                        { "id": "store", "shape": "ellipse", "label": "WAL Store" }
                    ],
                    "edges": [
                        { "from": "api", "to": "core", "label": "route" },
                        { "from": "core", "to": "store", "label": "append" }
                    ],
                    "frames": [
                        { "name": "Backend Plane", "includes": ["core", "store"] }
                    ]
                }
            },
            { "type": "h2", "seq_auto": true, "text": "Benchmark Comparison" },
            {
                "type": "table",
                "header_rows": 1,
                "col_widths": [220, 180, 240],
                "rows": [
                    [{ "text": "Component" }, { "text": "Latency" }, { "text": "Status" }],
                    [{ "text": "Gateway" }, { "text": "4ms", "valign": "middle" }, { "text": "Ready", "bg": "light-green", "valign": "middle" }],
                    [{ "text": "Indexer" }, { "text": "11ms", "valign": "middle" }, { "text": "Testing", "bg": "light-yellow", "valign": "middle" }]
                ]
            },
            { "type": "image", "src": img_file.to_str().unwrap(), "width": 120.0, "align": "center", "caption": "Badge Asset" },
            { "type": "code", "lang": "rust", "caption": "Core tick loop", "text": "pub fn tick() -> u64 { 42 }" }
        ]
    });
    let spec_path = work.join("blueprint.json");
    std::fs::write(&spec_path, serde_json::to_string_pretty(&spec).unwrap()).unwrap();

    let (c, created, err) = qmd_bin(&["--lib", lib_s, "--json", "--strict", "doc", "create", "--folder", "Design RFCs", "--spec", spec_path.to_str().unwrap()]);
    assert_eq!(c, 0, "doc create failed: {created}\nstderr={err}");
    let doc_id = created["data"]["id"].as_str().unwrap().to_string();
    let mtime = created["data"]["mtime_secs"].as_u64().unwrap();

    // 5. Inspect outline, spec, xml, md
    let (c, got, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "get", &doc_id, "--format", "outline"]);
    assert_eq!(c, 0, "{got}");
    assert!(got["data"]["blocks"].as_array().unwrap().len() >= 8);

    // 6. Dry-run apply + optimistic concurrency check
    let (c, dr_v, _) = qmd_bin(&["--lib", lib_s, "--json", "--dry-run", "doc", "apply", &doc_id, "--spec", spec_path.to_str().unwrap(), "--if-unmodified", &mtime.to_string()]);
    assert_eq!(c, 0, "{dr_v}");
    let (c_bad, v_bad, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "apply", &doc_id, "--spec", spec_path.to_str().unwrap(), "--if-unmodified", "123"]);
    assert_ne!(c_bad, 0);
    assert_eq!(v_bad["error"]["code"], "MODIFIED_EXTERNALLY");

    // 7. Fine-grained block, table and meta mutations via Core
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "insert", &doc_id, "--at", "last", "--type", "p", "--text", "Throughput reached 95k ops/sec cleanly."]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "style", &doc_id, "--at", "last", "--match", "95k ops/sec", "--bold", "true", "--text-color", "blue", "--bg-color", "light-blue"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "toggle-todo", &doc_id, "--at", "2.col1.0"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "table", "insert-row", &doc_id, "--table", "5", "--at", "3"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "table", "set-cell", &doc_id, "--table", "5", "--row", "3", "--col", "0", "--text", "Replicator", "--bg", "light-gray"]);
    assert_eq!(c, 0);

    // Verify block style & todo toggle persisted cleanly
    let (c, after_spec, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "get", &doc_id]);
    assert_eq!(c, 0);
    assert_eq!(after_spec["data"]["spec"]["blocks"][2]["columns"][1]["children"][0]["done"], true);

    // 8. Standalone board build -> PNG + standalone board create + export-svg + render-png
    let board_png = work.join("arch.png");
    let board_spec_file = work.join("diag.json");
    std::fs::write(&board_spec_file, serde_json::to_string_pretty(&spec["blocks"][3]["diagram"]).unwrap()).unwrap();
    let (c, b_res, _) = qmd_bin(&["--json", "board", "build", "--spec", board_spec_file.to_str().unwrap(), "--out", board_png.to_str().unwrap(), "--scale", "2.0"]);
    assert_eq!(c, 0, "{b_res}");
    let png_data = std::fs::read(&board_png).unwrap();
    assert!(png_data.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(png_data.len() > 2000);

    let (c, sb, _) = qmd_bin(&["--lib", lib_s, "--json", "board", "create", "Topology", "--spec", board_spec_file.to_str().unwrap()]);
    assert_eq!(c, 0, "{sb}");
    let sb_png = work.join("topo.png");
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "board", "render-png", sb["data"]["id"].as_str().unwrap(), "--out", sb_png.to_str().unwrap()]);
    assert_eq!(c, 0);
    assert!(std::fs::read(&sb_png).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));

    // 9. Theme create / set-var / select / lint / reset (and preserve preferences keys!)
    let prefs_file = lib.join(".quiet/preferences.json");
    std::fs::write(&prefs_file, r#"{"language":"en","vim_mode":true}"#).unwrap();
    let (c, t_c, _) = qmd_bin(&["--lib", lib_s, "--json", "theme", "create", "editorial", "--from", "craft", "--name", "Editorial Blue"]);
    assert_eq!(c, 0, "{t_c}");
    let (c, t_sv, _) = qmd_bin(&["--lib", lib_s, "--json", "theme", "set-var", "editorial", "--color-blue", "#1d4ed8"]);
    assert_eq!(c, 0, "{t_sv}");
    let (c, t_lint, _) = qmd_bin(&["--lib", lib_s, "--json", "theme", "lint", "editorial"]);
    assert_eq!(c, 0, "{t_lint}");
    let (c, t_sel, _) = qmd_bin(&["--lib", lib_s, "--json", "theme", "select", "editorial"]);
    assert_eq!(c, 0, "{t_sel}");
    let prefs_after: Value = serde_json::from_str(&std::fs::read_to_string(&prefs_file).unwrap()).unwrap();
    assert_eq!(prefs_after["theme"], "editorial");
    assert_eq!(prefs_after["feishu"], false);
    assert_eq!(prefs_after["language"], "en");
    assert_eq!(prefs_after["vim_mode"], true);

    // 10. Validate strict passes cleanly on curated document & catches aesthetic defects on bad doc
    let (c, val_ok, _) = qmd_bin(&["--lib", lib_s, "--json", "--strict", "validate", "doc", &doc_id]);
    assert_eq!(c, 0, "expected strict pass: {val_ok}");

    let bad_spec = json!({
        "title": "Bad Doc",
        "blocks": [
            { "type": "h1", "text": "Intro" },
            { "type": "h4", "text": "Skipped h2 and h3!" },
            { "type": "callout", "bg": "light-gray", "border": "gray", "children": [{ "type": "p", "text": "First callout" }] },
            { "type": "callout", "bg": "light-blue", "border": "blue", "children": [{ "type": "p", "text": "Stacked second callout" }] },
            { "type": "quote", "runs": [{ "text": "Painted quote", "text_color": "blue" }] },
            {
                "type": "grid",
                "columns": [
                    { "ratio": 0.5, "children": [] },
                    { "ratio": 0.5, "children": [{ "type": "p", "text": "Non-empty" }] }
                ]
            },
            {
                "type": "board",
                "height": 300,
                "diagram": {
                    "layout": "flow-lr",
                    "theme": "slate",
                    "nodes": [
                        { "id": "a1", "label": "Node A1" },
                        { "id": "a2", "label": "Node A2" },
                        { "id": "b1", "label": "Node B1" },
                        { "id": "b2", "label": "Node B2" },
                        { "id": "b3", "label": "Node B3" }
                    ],
                    "edges": [
                        { "from": "a1", "to": "a2" },
                        { "from": "b1", "to": "b2" },
                        { "from": "b2", "to": "b3" }
                    ],
                    "frames": [
                        { "name": "Frame A", "includes": ["a1", "a2"] },
                        { "name": "Frame B", "includes": ["b1", "b2", "b3"] }
                    ]
                }
            }
        ]
    });
    let (c, bad_created, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "create", "--spec", &bad_spec.to_string()]);
    assert_eq!(c, 0);
    let bad_id = bad_created["data"]["id"].as_str().unwrap().to_string();
    let (c_strict_fail, val_bad, _) = qmd_bin(&["--lib", lib_s, "--json", "--strict", "validate", "doc", &bad_id]);
    assert_ne!(c_strict_fail, 0);
    let rules: Vec<&str> = val_bad["diagnostics"].as_array().unwrap().iter().filter_map(|d| d["rule"].as_str()).collect();
    assert!(rules.contains(&"heading-hierarchy"), "{rules:?}");
    assert!(rules.contains(&"consecutive-callouts"), "{rules:?}");
    assert!(rules.contains(&"quote-color-restraint"), "{rules:?}");
    assert!(rules.contains(&"grid-balance"), "{rules:?}");
    assert!(rules.contains(&"board-frame-overlap"), "{rules:?}");

    // 11. Asset verification + headless exports (HTML, Markdown, Preview)
    let (c, aver, _) = qmd_bin(&["--lib", lib_s, "--json", "asset", "verify"]);
    assert_eq!(c, 0, "{aver}");
    assert_eq!(aver["data"]["missing_count"], 0);

    let html_out = work.join("out.html");
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "export", "html", &doc_id, "--out", html_out.to_str().unwrap()]);
    assert_eq!(c, 0);
    let html_body = std::fs::read_to_string(&html_out).unwrap();
    assert!(html_body.contains("<svg"));
    assert!(html_body.contains("<math"));
    assert!(html_body.contains("data:image/"));

    let md_out = work.join("out.md");
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "export", "markdown", &doc_id, "--out", md_out.to_str().unwrap(), "--assets-dir", "assets"]);
    assert_eq!(c, 0);
    assert!(work.join("out.md").is_file());
    assert!(work.join("assets").is_dir());

    let preview_dir = work.join("preview");
    let (c, prev_res, _) = qmd_bin(&["--lib", lib_s, "--json", "export", "preview", &doc_id, "--out-dir", preview_dir.to_str().unwrap()]);
    assert_eq!(c, 0, "{prev_res}");
    assert!(preview_dir.join("index.html").is_file());
    assert_eq!(prev_res["data"]["boards"].as_array().unwrap().len(), 1);
    let board_preview_png = PathBuf::from(prev_res["data"]["boards"][0]["png"].as_str().unwrap());
    assert!(std::fs::read(board_preview_png).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));

    // 12. Direct library API smoke test (`run_args`)
    let code = run_args(&["--lib".into(), lib_s.into(), "--quiet".into(), "validate".into(), "doc".into(), doc_id]);
    assert_eq!(code, 0);

    // 13. Versions & theme reset
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "versions", "save", &bad_id]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "delete", &bad_id]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "restore", &bad_id]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "theme", "reset", "editorial"]);
    assert_eq!(c, 0);
}

#[test]
fn fine_grained_core_ops_and_library_ops() {
    let tmp = TempDir::new("fine");
    let lib = tmp.path().join("lib");
    let lib_s = lib.to_str().unwrap();

    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "library", "init"]);
    assert_eq!(c, 0);
    let (c, f_out, _) = qmd_bin(&["--lib", lib_s, "--json", "folder", "create", "Notes"]);
    assert_eq!(c, 0, "{f_out}");
    let (c, d, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "create", "--title", "Playbook"]);
    assert_eq!(c, 0, "{d}");
    let doc_id = d["data"]["id"].as_str().unwrap().to_string();

    // Meta set / cover / icon / get
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "meta", "set", &doc_id, "--width", "full", "--catalogue-open", "true", "--info-hidden", "true"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "meta", "icon", "set", &doc_id, "--emoji", "memo"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "meta", "cover", "set", &doc_id, "--gallery", "gradient-1", "--y", "-0.3"]);
    assert_eq!(c, 0);
    let (c, mg, _) = qmd_bin(&["--lib", lib_s, "--json", "meta", "get", &doc_id]);
    assert_eq!(c, 0, "{mg}");
    assert_eq!(mg["data"]["width"], "full");
    assert_eq!(mg["data"]["icon"], "📝");

    // Block fine-grained ops: insert, turn-into, inline, fold, indent/outdent, move, delete
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "update", &doc_id, "--at", "0", "--text", "Section One"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "turn-into", &doc_id, "--at", "0", "--to", "h1"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "insert", &doc_id, "--at", "0", "--type", "p", "--text", "Formula: "]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "inline", &doc_id, "--at", "1", "--kind", "equation", "--latex", "a^2+b^2=c^2"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "insert", &doc_id, "--at", "1", "--type", "bullet", "--text", "Parent item"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "insert", &doc_id, "--at", "2", "--type", "bullet", "--text", "Child item"]);
    assert_eq!(c, 0);
    let (c, r1, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "indent", &doc_id, "--at", "3"]);
    assert_eq!(c, 0, "{r1}");
    let (c, r2, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "fold", &doc_id, "--at", "2", "--folded", "true"]);
    assert_eq!(c, 0, "{r2}");
    let (c, r_uf, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "fold", &doc_id, "--at", "2", "--folded", "false"]);
    assert_eq!(c, 0, "{r_uf}");
    let (c, r3, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "outdent", &doc_id, "--at", "2.0"]);
    assert_eq!(c, 0, "indent={r1}\nfold={r2}\nunfold={r_uf}\noutdent={r3}");
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "move", &doc_id, "--at", "3", "--to", "1", "--place", "after"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "block", "delete", &doc_id, "--at", "last"]);
    assert_eq!(c, 0);

    // Table fine-grained ops
    let (c, tc, _) = qmd_bin(&["--lib", lib_s, "--json", "table", "create", &doc_id, "--rows", "3", "--cols", "3", "--widths", "140,200,140", "--header-rows", "1"]);
    assert_eq!(c, 0, "{tc}");
    let tloc = tc["data"]["table_loc"].as_str().unwrap().to_string();
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "table", "insert-col", &doc_id, "--table", &tloc, "--at", "2"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "table", "merge", &doc_id, "--table", &tloc, "--range", "1,0:2,1"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "table", "unmerge", &doc_id, "--table", &tloc, "--row", "1", "--col", "0"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "table", "distribute", &doc_id, "--table", &tloc]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "table", "set-header", &doc_id, "--table", &tloc, "--header-row", "true", "--header-col", "true"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "table", "delete-cols", &doc_id, "--table", &tloc, "--col", "3"]);
    assert_eq!(c, 0);

    // Search, duplicate, rename, move, validate library, export docxml & harness script
    let (c, sres, _) = qmd_bin(&["--lib", lib_s, "--json", "library", "search", "Section"]);
    assert_eq!(c, 0, "{sres}");
    assert_eq!(sres["data"].as_array().unwrap().len(), 1);
    let (c, dup, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "duplicate", &doc_id, "--title", "Playbook Copy", "--folder", "Notes"]);
    assert_eq!(c, 0, "{dup}");
    let copy_id = dup["data"]["id"].as_str().unwrap().to_string();
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "rename", &copy_id, "Playbook V2"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "versions", "save", &copy_id]);
    assert_eq!(c, 0);
    let (c, vlist, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "versions", "list", &copy_id]);
    assert_eq!(c, 0, "{vlist}");
    let vname = vlist["data"]["versions"][0]["name"].as_str().unwrap().to_string();
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "versions", "label", &copy_id, "--version", &vname, "--label", "Initial"]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "versions", "diff", &copy_id, "--version", &vname]);
    assert_eq!(c, 0);
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "doc", "versions", "restore", &copy_id, "--version", &vname]);
    assert_eq!(c, 0);
    let (c, x_res, _) = qmd_bin(&["--lib", lib_s, "--json", "export", "docxml", &doc_id]);
    assert_eq!(c, 0, "{x_res}");
    assert!(x_res["data"]["xml"].as_str().unwrap().contains("<title"));
    assert!(x_res["data"]["xml"].as_str().unwrap().contains("<table"));
    let script_path = tmp.path().join("shot.txt");
    let (c, _, _) = qmd_bin(&["--lib", lib_s, "--json", "export", "harness-script", &doc_id, "--out", script_path.to_str().unwrap()]);
    assert_eq!(c, 0);
    assert!(script_path.is_file());
    let (c, vlib, _) = qmd_bin(&["--lib", lib_s, "--json", "validate", "library"]);
    assert_eq!(c, 0, "{vlib}");
    assert_eq!(vlib["data"]["errors"], 0);
}
