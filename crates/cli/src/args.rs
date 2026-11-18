//! Lightweight, self-describing CLI argument parser & command registry.
//! Both `--help` output and `qmd schema commands` derive from `CATALOG`.

use crate::output::{CliFailure, GlobalOpts};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Serialize)]
pub struct FlagSpec {
    pub name: &'static str,
    pub value: Option<&'static str>, // None = boolean switch
    pub required: bool,
    pub desc: &'static str,
}

#[derive(Clone, Serialize)]
pub struct CmdSpec {
    pub path: &'static str,
    pub summary: &'static str,
    pub positionals: &'static [&'static str],
    pub flags: &'static [FlagSpec],
    pub examples: &'static [&'static str],
}

const fn fl(name: &'static str, value: Option<&'static str>, desc: &'static str) -> FlagSpec {
    FlagSpec { name, value, required: false, desc }
}

const fn req(name: &'static str, value: &'static str, desc: &'static str) -> FlagSpec {
    FlagSpec { name, value: Some(value), required: true, desc }
}

pub const TOP_GROUPS: &[(&str, &str)] = &[
    ("schema", "Inspect JSON schemas, palette colors, code langs, built-in covers and commands"),
    ("library", "Initialize, inspect tree and search a Quiet.md library"),
    ("folder", "Create, rename, move, reorder or trash folders"),
    ("doc", "Create, declaratively apply specs, read with block locators, manage versions"),
    ("meta", "Control page width, pinning, stars, reading mode, info row, icon & cover"),
    ("block", "Fine-grained block insert, update, inline styling, inline objects & move"),
    ("table", "Fine-grained table creation, cell styling, merges, widths & row/col ops"),
    ("board", "Build semantic Excalidraw diagrams, inspect, export SVG or render PNG"),
    ("asset", "Import and verify images & file attachments in .assets/"),
    ("theme", "List, inspect, select, clone, customize and lint CSS themes"),
    ("validate", "Run structural invariants + aesthetic layout linter on docs or library"),
    ("export", "Headless export to HTML, Markdown, DocxXML, PNG preview or harness script"),
];

pub const CATALOG: &[CmdSpec] = &[
    CmdSpec {
        path: "schema",
        summary: "Output machine-readable schema/reference data (all|commands|doc-spec|board-spec|palette|langs|covers|themes)",
        positionals: &["[topic]"],
        flags: &[],
        examples: &["qmd schema palette --json", "qmd schema doc-spec --json", "qmd schema covers --json"],
    },
    CmdSpec {
        path: "library init",
        summary: "Initialize an empty FORMAT 4 library at --lib / QUIET_MD_HOME",
        positionals: &[],
        flags: &[fl("with-guides", None, "Reserved option; creates a clean empty library")],
        examples: &["qmd --lib /tmp/my-lib library init --json"],
    },
    CmdSpec {
        path: "library status",
        summary: "Show library path, format version, doc/folder counts and active theme",
        positionals: &[],
        flags: &[],
        examples: &["qmd library status --json"],
    },
    CmdSpec {
        path: "library tree",
        summary: "List folders and documents in sidebar/tree order with metadata",
        positionals: &[],
        flags: &[fl("flat", None, "Return flat list instead of nested tree")],
        examples: &["qmd library tree --json"],
    },
    CmdSpec {
        path: "library search",
        summary: "Full-text search across titles, bodies and board text",
        positionals: &["<query>"],
        flags: &[fl("limit", Some("N"), "Maximum hits (default 20)")],
        examples: &["qmd library search \"架构\" --json"],
    },
    CmdSpec {
        path: "folder create",
        summary: "Create a folder on disk and in .quiet/index.json",
        positionals: &["<title>"],
        flags: &[fl("parent", Some("folder"), "Parent folder id, path or title")],
        examples: &["qmd folder create \"架构设计\" --json"],
    },
    CmdSpec {
        path: "folder rename",
        summary: "Rename a folder",
        positionals: &["<folder>", "<title>"],
        flags: &[],
        examples: &["qmd folder rename \"Old\" \"New\" --json"],
    },
    CmdSpec {
        path: "folder move",
        summary: "Move a folder under another parent (or root) and/or reorder before sibling",
        positionals: &["<folder>"],
        flags: &[fl("parent", Some("folder"), "Target parent folder (omit or 'root' for top-level)"), fl("before", Some("entry"), "Place before sibling id/path/title")],
        examples: &["qmd folder move \"RFCs\" --parent \"Engineering\" --json"],
    },
    CmdSpec {
        path: "folder delete",
        summary: "Move a folder and its contents to Trash",
        positionals: &["<folder>"],
        flags: &[],
        examples: &["qmd folder delete \"Archive\" --json"],
    },
    CmdSpec {
        path: "doc create",
        summary: "Create a new document (empty, or from --spec JSON, --xml DocxXML, or --md Markdown)",
        positionals: &[],
        flags: &[
            fl("title", Some("text"), "Document title (when not provided in spec/xml)"),
            fl("folder", Some("folder"), "Parent folder id, path or title"),
            fl("spec", Some("file|-"), "Declarative DocSpec JSON file or '-' for stdin"),
            fl("xml", Some("file|-"), "DocxXML file or '-' for stdin"),
            fl("md", Some("file|-"), "Markdown file or '-' for stdin (creates .xml by default unless --as-markdown)"),
            fl("as-markdown", None, "Store as .md document instead of rich DocxXML .xml"),
            fl("strict", None, "Fail if aesthetic/structural linter reports warnings or errors"),
        ],
        examples: &["qmd doc create --folder \"Engineering\" --spec spec.json --json"],
    },
    CmdSpec {
        path: "doc apply",
        summary: "Atomically update/replace an existing document from a DocSpec JSON (--spec) or DocxXML (--xml)",
        positionals: &["<doc>"],
        flags: &[
            fl("spec", Some("file|-"), "DocSpec JSON file or '-' for stdin"),
            fl("xml", Some("file|-"), "DocxXML file or '-' for stdin"),
            fl("if-unmodified", Some("unix_secs"), "Abort if file mtime changed externally"),
            fl("strict", None, "Fail if linter reports warnings or errors"),
        ],
        examples: &["qmd doc apply \"Q4 Plan\" --spec updated.json --dry-run --json"],
    },
    CmdSpec {
        path: "doc get",
        summary: "Inspect a document as structured DocSpec JSON (with `loc` per block), DocxXML, or Markdown",
        positionals: &["<doc>"],
        flags: &[fl("format", Some("spec|xml|md|outline"), "Output representation (default spec)")],
        examples: &["qmd doc get \"Q4 Plan\" --json", "qmd doc get \"Q4 Plan\" --format xml"],
    },
    CmdSpec {
        path: "doc rename",
        summary: "Rename a document (updates title and filename cleanly)",
        positionals: &["<doc>", "<title>"],
        flags: &[],
        examples: &["qmd doc rename \"Draft\" \"Final Spec\" --json"],
    },
    CmdSpec {
        path: "doc move",
        summary: "Move a document into a folder and/or reorder among siblings",
        positionals: &["<doc>"],
        flags: &[fl("folder", Some("folder"), "Target folder (or 'root')"), fl("before", Some("entry"), "Place before sibling")],
        examples: &["qmd doc move \"Final Spec\" --folder \"Engineering\" --json"],
    },
    CmdSpec {
        path: "doc duplicate",
        summary: "Duplicate a document (preserves content and metadata into target folder)",
        positionals: &["<doc>"],
        flags: &[fl("title", Some("text"), "New copy title"), fl("folder", Some("folder"), "Target folder")],
        examples: &["qmd doc duplicate \"Template\" --title \"Week 42\" --json"],
    },
    CmdSpec {
        path: "doc delete",
        summary: "Move a document to Trash",
        positionals: &["<doc>"],
        flags: &[],
        examples: &["qmd doc delete \"Old Note\" --json"],
    },
    CmdSpec {
        path: "doc restore",
        summary: "Restore a trashed document by id",
        positionals: &["<id>"],
        flags: &[],
        examples: &["qmd doc restore <uuid> --json"],
    },
    CmdSpec {
        path: "doc purge",
        summary: "Permanently purge a trashed document by id",
        positionals: &["<id>"],
        flags: &[],
        examples: &["qmd doc purge <uuid> --json"],
    },
    CmdSpec {
        path: "doc versions",
        summary: "Manage document version history snapshots",
        positionals: &["<list|save|label|diff|restore>", "<doc>"],
        flags: &[fl("version", Some("name"), "Snapshot filename for label/diff/restore"), fl("label", Some("text"), "Milestone name (empty clears)")],
        examples: &["qmd doc versions list \"Q4 Plan\" --json", "qmd doc versions save \"Q4 Plan\" --json"],
    },
    CmdSpec {
        path: "meta get",
        summary: "Get document metadata (width, pinned, starred, icon, cover, info row, reading mode)",
        positionals: &["<doc>"],
        flags: &[],
        examples: &["qmd meta get \"Q4 Plan\" --json"],
    },
    CmdSpec {
        path: "meta set",
        summary: "Update document metadata flags",
        positionals: &["<doc>"],
        flags: &[
            fl("width", Some("standard|wide|full"), "Page content column width"),
            fl("pinned", Some("bool"), "Pin in sidebar"),
            fl("starred", Some("bool"), "Favorite / star document"),
            fl("reading", Some("bool"), "Reading mode"),
            fl("catalogue-open", Some("bool"), "Keep document outline open"),
            fl("refs-hidden", Some("bool"), "Hide backlinks list"),
            fl("info-hidden", Some("bool"), "Hide doc info row under title"),
            fl("info-created", Some("bool"), "Show created time in info row"),
            fl("info-modified", Some("bool"), "Show modified time in info row"),
            fl("order", Some("u64"), "Sibling order inside folder"),
            fl("root-order", Some("u64"), "Top-level sidebar order"),
        ],
        examples: &["qmd meta set \"Q4 Plan\" --width wide --pinned true --catalogue-open true --json"],
    },
    CmdSpec {
        path: "meta cover",
        summary: "Set or remove document banner cover",
        positionals: &["<set|remove>", "<doc>"],
        flags: &[
            fl("gallery", Some("name"), "Built-in gallery cover (e.g. aurora-2, gradient-5)"),
            fl("file", Some("path"), "Local image file to import into .assets/"),
            fl("x", Some("f32"), "Horizontal crop offset ratio (-1..1, default 0)"),
            fl("y", Some("f32"), "Vertical crop offset ratio (-1..1, default 0)"),
        ],
        examples: &["qmd meta cover set \"Q4 Plan\" --gallery aurora-2 --y 0.3 --json"],
    },
    CmdSpec {
        path: "meta icon",
        summary: "Set or remove document emoji icon",
        positionals: &["<set|remove>", "<doc>"],
        flags: &[fl("emoji", Some("char|id"), "Emoji character or emoji-mart id (e.g. rocket, bulb)")],
        examples: &["qmd meta icon set \"Q4 Plan\" --emoji rocket --json"],
    },
    CmdSpec {
        path: "block insert",
        summary: "Insert one or more blocks via `editor::Core` relative to `--at <loc>`",
        positionals: &["<doc>"],
        flags: &[
            fl("at", Some("loc"), "Anchor block locator (default 'last')"),
            fl("place", Some("after|before|first-child"), "Placement relative to anchor (default after)"),
            fl("spec", Some("file|-|json"), "BlockSpec JSON (object or array), file path, or '-' for stdin"),
            fl("type", Some("kind"), "Typed quick insert (p|h1..h9|bullet|ordered|todo|code|quote|callout|divider|image|file|countdown|sub_page_list)"),
            fl("text", Some("str"), "Text content for quick insert"),
            fl("lang", Some("name"), "Code language"),
            fl("caption", Some("str"), "Code or image caption"),
            fl("wrap", None, "Enable code line wrap"),
            fl("emoji", Some("char|id"), "Callout emoji"),
            fl("bg", Some("color"), "Block or callout background color"),
            fl("border", Some("hue"), "Callout border color"),
            fl("text-color", Some("color"), "Callout text color"),
            fl("align", Some("left|center|right"), "Block alignment"),
            fl("seq-auto", None, "Enable auto-numbering on heading"),
            fl("done", None, "Mark todo done"),
            fl("src", Some("path|url"), "Image or attachment path"),
            fl("width", Some("px"), "Image width"),
            fl("view", Some("card|preview"), "File view mode"),
        ],
        examples: &["qmd block insert \"Q4 Plan\" --at 0 --type callout --emoji bulb --bg light-blue --border blue --text \"Summary\" --json"],
    },
    CmdSpec {
        path: "block update",
        summary: "Update block content/attributes at `--at <loc>` via `editor::Core`",
        positionals: &["<doc>"],
        flags: &[
            req("at", "loc", "Target block locator"),
            fl("spec", Some("file|-|json"), "Full replacement BlockSpec JSON"),
            fl("text", Some("str"), "Replace plain text"),
            fl("emoji", Some("char|id"), "Update callout emoji"),
            fl("bg", Some("color"), "Update callout/block background (use 'none' to clear)"),
            fl("border", Some("hue"), "Update callout border (use 'none' to clear)"),
            fl("text-color", Some("color"), "Update callout text color"),
            fl("lang", Some("name"), "Update code block language"),
            fl("caption", Some("str"), "Update code/image caption"),
            fl("wrap", Some("bool"), "Update code line wrapping"),
            fl("width", Some("px"), "Update image width"),
            fl("height", Some("px"), "Update image height"),
            fl("crop", Some("l,t,r,b"), "Update image crop fractions"),
            fl("seq-auto", Some("bool"), "Toggle heading auto-numbering"),
            fl("seq", Some("u32"), "Set explicit ordered list number"),
            fl("done", Some("bool"), "Set todo completion"),
            fl("view", Some("card|preview"), "Update file block view mode"),
            fl("align", Some("left|center|right"), "Update block alignment"),
        ],
        examples: &["qmd block update \"Q4 Plan\" --at 2 --bg light-green --align center --json"],
    },
    CmdSpec {
        path: "block style",
        summary: "Apply inline marks (bold, italic, underline, strike, code, link, colors) to whole block or substring/range",
        positionals: &["<doc>"],
        flags: &[
            req("at", "loc", "Target text block locator"),
            fl("range", Some("start..end"), "UTF-8 byte range inside block text"),
            fl("match", Some("substr"), "Apply marks to every occurrence of substring"),
            fl("bold", Some("bool"), "Set bold"),
            fl("italic", Some("bool"), "Set italic"),
            fl("underline", Some("bool"), "Set underline"),
            fl("strike", Some("bool"), "Set strikethrough"),
            fl("code", Some("bool"), "Set inline code"),
            fl("link", Some("url"), "Set link URL (empty clears)"),
            fl("text-color", Some("color"), "Set font color ('none' clears)"),
            fl("bg-color", Some("color"), "Set highlight color ('none' clears)"),
        ],
        examples: &["qmd block style \"Q4 Plan\" --at 1 --match \"6ms\" --bold true --text-color blue --bg-color light-yellow --json"],
    },
    CmdSpec {
        path: "block inline",
        summary: "Insert an inline object (equation, button, reminder, doc-link) at offset in a text block",
        positionals: &["<doc>"],
        flags: &[
            req("at", "loc", "Target text block locator"),
            req("kind", "equation|button|reminder|doc-link", "Inline object kind"),
            fl("offset", Some("byte"), "Insert byte offset (default end of block)"),
            fl("latex", Some("expr"), "LaTeX expression for equation"),
            fl("text", Some("label"), "Button text or doc-link display title"),
            fl("action", Some("OpenLink|DuplicatePage|FollowPage"), "Button action"),
            fl("src", Some("url"), "Button link URL"),
            fl("color", Some("name"), "Button background color"),
            fl("date", Some("YYYY-MM-DD"), "Reminder date"),
            fl("time", Some("HH:MM"), "Reminder time (omit for whole-day)"),
            fl("target", Some("doc"), "Target document for doc-link"),
        ],
        examples: &["qmd block inline \"Q4 Plan\" --at 1 --kind doc-link --target \"API Design\" --json"],
    },
    CmdSpec {
        path: "block turn-into",
        summary: "Convert a block to another type via `editor::Core::turn_into`",
        positionals: &["<doc>"],
        flags: &[req("at", "loc", "Target block locator"), req("to", "p|h1..h9|bullet|ordered|todo|code|quote|callout", "Target block type"), fl("lang", Some("name"), "Language when converting to code")],
        examples: &["qmd block turn-into \"Q4 Plan\" --at 3 --to h2 --json"],
    },
    CmdSpec {
        path: "block move",
        summary: "Move a block before/after/into another block via `editor::Core::move_blocks`",
        positionals: &["<doc>"],
        flags: &[req("at", "loc", "Source block locator"), req("to", "loc", "Target anchor block locator"), fl("place", Some("after|before|first-child"), "Placement (default after)")],
        examples: &["qmd block move \"Q4 Plan\" --at 5 --to 1 --place after --json"],
    },
    CmdSpec {
        path: "block indent",
        summary: "Indent a block under its previous sibling",
        positionals: &["<doc>"],
        flags: &[req("at", "loc", "Target block locator")],
        examples: &["qmd block indent \"Q4 Plan\" --at 4 --json"],
    },
    CmdSpec {
        path: "block outdent",
        summary: "Outdent a nested block up one level",
        positionals: &["<doc>"],
        flags: &[req("at", "loc", "Target block locator")],
        examples: &["qmd block outdent \"Q4 Plan\" --at 3.0 --json"],
    },
    CmdSpec {
        path: "block fold",
        summary: "Set or toggle fold state on a heading/list/paragraph",
        positionals: &["<doc>"],
        flags: &[req("at", "loc", "Target block locator"), fl("folded", Some("bool"), "Explicit fold state (omit to toggle)")],
        examples: &["qmd block fold \"Q4 Plan\" --at 2 --folded true --json"],
    },
    CmdSpec {
        path: "block toggle-todo",
        summary: "Toggle a todo checkbox",
        positionals: &["<doc>"],
        flags: &[req("at", "loc", "Target todo block locator")],
        examples: &["qmd block toggle-todo \"Q4 Plan\" --at 6 --json"],
    },
    CmdSpec {
        path: "block delete",
        summary: "Delete a block via `editor::Core::delete_block`",
        positionals: &["<doc>"],
        flags: &[req("at", "loc", "Target block locator")],
        examples: &["qmd block delete \"Q4 Plan\" --at 7 --json"],
    },
    CmdSpec {
        path: "table create",
        summary: "Insert a new table via `editor::Core`",
        positionals: &["<doc>"],
        flags: &[
            fl("at", Some("loc"), "Anchor block locator (default last)"),
            fl("place", Some("after|before|first-child"), "Placement (default after)"),
            fl("rows", Some("N"), "Row count (default 3)"),
            fl("cols", Some("N"), "Column count (default 3)"),
            fl("widths", Some("w1,w2,..."), "Comma-separated column widths in px"),
            fl("header-rows", Some("N"), "Header row count (default 1)"),
            fl("header-col", Some("bool"), "First column is header column"),
        ],
        examples: &["qmd table create \"Q4 Plan\" --rows 4 --cols 3 --widths 180,280,140 --header-rows 1 --json"],
    },
    CmdSpec {
        path: "table set-cell",
        summary: "Set cell text/blocks, background color and vertical alignment",
        positionals: &["<doc>"],
        flags: &[
            req("table", "loc", "Table locator"),
            req("row", "r", "0-based row index"),
            req("col", "c", "0-based column index"),
            fl("text", Some("str"), "Cell plain text"),
            fl("spec", Some("file|-|json"), "Rich BlockSpec array for multi-block cell content"),
            fl("bg", Some("color"), "Cell background color ('none' clears)"),
            fl("valign", Some("top|middle|bottom"), "Vertical alignment ('none' clears)"),
        ],
        examples: &["qmd table set-cell \"Q4 Plan\" --table 4 --row 1 --col 2 --text \"Done\" --bg light-green --valign middle --json"],
    },
    CmdSpec {
        path: "table set-widths",
        summary: "Set column widths of a table (all via comma list, or single --col/--width)",
        positionals: &["<doc>"],
        flags: &[
            req("table", "loc", "Table locator"),
            fl("widths", Some("w1,w2,..."), "All column widths in px"),
            fl("col", Some("c"), "Single 0-based column index"),
            fl("width", Some("px"), "Width for --col"),
        ],
        examples: &["qmd table set-widths \"Q4 Plan\" --table 4 --widths 160,340,160 --json"],
    },
    CmdSpec {
        path: "table distribute",
        summary: "Distribute all column widths equally",
        positionals: &["<doc>"],
        flags: &[req("table", "loc", "Table locator")],
        examples: &["qmd table distribute \"Q4 Plan\" --table 4 --json"],
    },
    CmdSpec {
        path: "table merge",
        summary: "Merge a rectangular cell range (r1,c1..r2,c2)",
        positionals: &["<doc>"],
        flags: &[req("table", "loc", "Table locator"), req("range", "r1,c1:r2,c2", "Inclusive cell rectangle")],
        examples: &["qmd table merge \"Q4 Plan\" --table 4 --range 1,0:2,1 --json"],
    },
    CmdSpec {
        path: "table unmerge",
        summary: "Unmerge a merged cell at (row,col)",
        positionals: &["<doc>"],
        flags: &[req("table", "loc", "Table locator"), req("row", "r", "Row"), req("col", "c", "Column")],
        examples: &["qmd table unmerge \"Q4 Plan\" --table 4 --row 1 --col 0 --json"],
    },
    CmdSpec {
        path: "table insert-row",
        summary: "Insert a row before index --at",
        positionals: &["<doc>"],
        flags: &[req("table", "loc", "Table locator"), fl("at", Some("r"), "0-based index (default append)")],
        examples: &["qmd table insert-row \"Q4 Plan\" --table 4 --at 2 --json"],
    },
    CmdSpec {
        path: "table insert-col",
        summary: "Insert a column before index --at",
        positionals: &["<doc>"],
        flags: &[req("table", "loc", "Table locator"), fl("at", Some("c"), "0-based index (default append)")],
        examples: &["qmd table insert-col \"Q4 Plan\" --table 4 --at 2 --json"],
    },
    CmdSpec {
        path: "table delete-rows",
        summary: "Delete row range [from..to] (inclusive-start, exclusive-end)",
        positionals: &["<doc>"],
        flags: &[req("table", "loc", "Table locator"), req("row", "r", "Start row (or r1..r2)")],
        examples: &["qmd table delete-rows \"Q4 Plan\" --table 4 --row 2 --json"],
    },
    CmdSpec {
        path: "table delete-cols",
        summary: "Delete column index or range",
        positionals: &["<doc>"],
        flags: &[req("table", "loc", "Table locator"), req("col", "c", "Start col (or c1..c2)")],
        examples: &["qmd table delete-cols \"Q4 Plan\" --table 4 --col 2 --json"],
    },
    CmdSpec {
        path: "table set-header",
        summary: "Toggle header row or header column",
        positionals: &["<doc>"],
        flags: &[req("table", "loc", "Table locator"), fl("header-row", Some("bool"), "Enable/disable top header row"), fl("header-col", Some("bool"), "Enable/disable left header column")],
        examples: &["qmd table set-header \"Q4 Plan\" --table 4 --header-row true --header-col false --json"],
    },
    CmdSpec {
        path: "board build",
        summary: "Compile a DiagramSpec JSON into standalone .excalidraw JSON, SVG, or PNG",
        positionals: &[],
        flags: &[
            req("spec", "file|-", "DiagramSpec JSON file or '-' for stdin"),
            fl("out", Some("path"), "Output file (.excalidraw | .svg | .png; default stdout .excalidraw)"),
            fl("format", Some("excalidraw|svg|png"), "Output format (auto-detected from --out extension)"),
            fl("scale", Some("f32"), "PNG rasterization scale (default 2.0)"),
        ],
        examples: &["qmd board build --spec arch.json --out arch.png --json"],
    },
    CmdSpec {
        path: "board create",
        summary: "Create a standalone .excalidraw board file in the library",
        positionals: &["<title>"],
        flags: &[req("spec", "file|-", "DiagramSpec JSON or raw .excalidraw file"), fl("folder", Some("folder"), "Target folder")],
        examples: &["qmd board create \"System Map\" --spec arch.json --json"],
    },
    CmdSpec {
        path: "board insert-into-doc",
        summary: "Compile a DiagramSpec JSON and insert/replace an inline <whiteboard> block in a document",
        positionals: &["<doc>"],
        flags: &[
            req("spec", "file|-", "DiagramSpec JSON file or '-' for stdin"),
            fl("at", Some("loc"), "Target anchor (or whiteboard loc when --replace)"),
            fl("place", Some("after|before|first-child"), "Placement (default after)"),
            fl("replace", None, "Replace an existing whiteboard block at --at"),
            fl("height", Some("px"), "Board height in document page (default from spec or 400)"),
        ],
        examples: &["qmd board insert-into-doc \"Q4 Plan\" --spec arch.json --at 2 --json"],
    },
    CmdSpec {
        path: "board inspect",
        summary: "Inspect elements, bounds and text of a standalone board or doc whiteboard block",
        positionals: &["<doc>"],
        flags: &[fl("at", Some("loc"), "Whiteboard block locator (omit if <doc> is a .excalidraw board)")],
        examples: &["qmd board inspect \"Q4 Plan\" --at 3 --json"],
    },
    CmdSpec {
        path: "board export-svg",
        summary: "Export a standalone board or doc whiteboard block to SVG",
        positionals: &["<doc>"],
        flags: &[fl("at", Some("loc"), "Whiteboard block locator"), fl("out", Some("path"), "Output .svg path (omit for stdout)")],
        examples: &["qmd board export-svg \"Q4 Plan\" --at 3 --out /tmp/board.svg --json"],
    },
    CmdSpec {
        path: "board render-png",
        summary: "Render a standalone board or doc whiteboard block to PNG using resvg",
        positionals: &["<doc>"],
        flags: &[fl("at", Some("loc"), "Whiteboard block locator"), req("out", "path", "Output .png path"), fl("scale", Some("f32"), "Scale factor (default 2.0)")],
        examples: &["qmd board render-png \"Q4 Plan\" --at 3 --out /tmp/board.png --json"],
    },
    CmdSpec {
        path: "asset add-image",
        summary: "Validate and import an image file into .assets/, returning its library-relative path and pixel dimensions",
        positionals: &["<file>"],
        flags: &[],
        examples: &["qmd asset add-image ./arch.png --json"],
    },
    CmdSpec {
        path: "asset add-file",
        summary: "Import an attachment file into .assets/files/<uuid>/",
        positionals: &["<file>"],
        flags: &[],
        examples: &["qmd asset add-file ./report.pdf --json"],
    },
    CmdSpec {
        path: "asset list",
        summary: "List files in .assets/",
        positionals: &[],
        flags: &[],
        examples: &["qmd asset list --json"],
    },
    CmdSpec {
        path: "asset verify",
        summary: "Verify all image, cover and attachment references across the library",
        positionals: &[],
        flags: &[],
        examples: &["qmd asset verify --json"],
    },
    CmdSpec {
        path: "theme list",
        summary: "List built-in and custom CSS themes and show active theme",
        positionals: &[],
        flags: &[],
        examples: &["qmd theme list --json"],
    },
    CmdSpec {
        path: "theme show",
        summary: "Inspect a theme's metadata, parsed rules and CSS source",
        positionals: &["<theme-id>"],
        flags: &[fl("raw", None, "Return raw CSS text")],
        examples: &["qmd theme show craft --json"],
    },
    CmdSpec {
        path: "theme props",
        summary: "List all valid theme CSS selectors, properties and --token-* names",
        positionals: &[],
        flags: &[],
        examples: &["qmd theme props --json"],
    },
    CmdSpec {
        path: "theme select",
        summary: "Select active theme in .quiet/preferences.json",
        positionals: &["<theme-id>"],
        flags: &[],
        examples: &["qmd theme select craft --json"],
    },
    CmdSpec {
        path: "theme create",
        summary: "Clone a built-in/existing theme into .quiet/themes/<id>.css",
        positionals: &["<new-id>"],
        flags: &[fl("from", Some("theme-id"), "Base theme to clone (default craft)"), fl("name", Some("str"), "Display name"), fl("css", Some("file|-"), "Custom CSS file instead of cloning")],
        examples: &["qmd theme create editorial --from craft --name \"Editorial\" --json"],
    },
    CmdSpec {
        path: "theme set-var",
        summary: "Set or update a CSS custom property (--accent, --theme-name, --color-blue, etc.) in a custom theme",
        positionals: &["<theme-id>", "<var-name>", "<value>"],
        flags: &[],
        examples: &["qmd theme set-var editorial --color-blue \"#1d4ed8\" --json"],
    },
    CmdSpec {
        path: "theme lint",
        summary: "Validate a theme CSS file and report unapplied selectors/properties",
        positionals: &["<theme-id|file>"],
        flags: &[],
        examples: &["qmd theme lint editorial --json"],
    },
    CmdSpec {
        path: "theme reset",
        summary: "Delete a custom theme or override copy under .quiet/themes/",
        positionals: &["<theme-id>"],
        flags: &[],
        examples: &["qmd theme reset craft --json"],
    },
    CmdSpec {
        path: "validate doc",
        summary: "Run Core structural invariants + aesthetic linter on a document",
        positionals: &["<doc>"],
        flags: &[fl("strict", None, "Return non-zero exit code if warnings or hints exist")],
        examples: &["qmd validate doc \"Q4 Plan\" --strict --json"],
    },
    CmdSpec {
        path: "validate library",
        summary: "Validate every document in the library",
        positionals: &[],
        flags: &[fl("strict", None, "Fail on any warning or hint")],
        examples: &["qmd validate library --json"],
    },
    CmdSpec {
        path: "export html",
        summary: "Export a self-contained HTML page reflecting active CSS theme, embedded assets, SVG boards & MathML",
        positionals: &["<doc>"],
        flags: &[fl("out", Some("path"), "Output .html file (omit for stdout)"), fl("theme", Some("id"), "Override theme id")],
        examples: &["qmd export html \"Q4 Plan\" --out /tmp/preview.html --json"],
    },
    CmdSpec {
        path: "export markdown",
        summary: "Export a document to Markdown (optionally copying assets beside output)",
        positionals: &["<doc>"],
        flags: &[fl("out", Some("path"), "Output .md file (omit for stdout)"), fl("assets-dir", Some("name"), "Relative assets folder beside --out")],
        examples: &["qmd export markdown \"Q4 Plan\" --out /tmp/plan.md --json"],
    },
    CmdSpec {
        path: "export docxml",
        summary: "Export resolved DocxXML of a document",
        positionals: &["<doc>"],
        flags: &[fl("out", Some("path"), "Output .xml file (omit for stdout)")],
        examples: &["qmd export docxml \"Q4 Plan\" --out /tmp/plan.xml --json"],
    },
    CmdSpec {
        path: "export preview",
        summary: "Render HTML + optional headless browser PNG screenshot (and board PNGs) for agent visual inspection",
        positionals: &["<doc>"],
        flags: &[req("out-dir", "dir", "Directory for index.html, boards/*.png, and page.png (if chromium found)"), fl("width", Some("px"), "Viewport width (default 1280)"), fl("height", Some("px"), "Viewport height (default 960)"), fl("theme", Some("id"), "Override theme id")],
        examples: &["qmd export preview \"Q4 Plan\" --out-dir /tmp/qmd-preview --json"],
    },
    CmdSpec {
        path: "export harness-script",
        summary: "Generate a tools/harness FDL_SCRIPT script to open & capture the document in desktop Quiet.md (macOS devtools build)",
        positionals: &["<doc>"],
        flags: &[req("out", "path", "Output script .txt path"), fl("shot", Some("name"), "Screenshot name (default 'doc')")],
        examples: &["qmd export harness-script \"Q4 Plan\" --out /tmp/shot.txt --json"],
    },
];

pub fn known_top_command(arg: &str) -> bool {
    TOP_GROUPS.iter().any(|(g, _)| *g == arg) || arg == "cli" || arg == "help"
}

pub struct ParsedCmd {
    pub command_path: String,
    pub positionals: Vec<String>,
    pub flags: HashMap<String, Vec<String>>,
    pub spec: &'static CmdSpec,
}

impl ParsedCmd {
    pub fn flag(&self, name: &str) -> Option<&str> {
        self.flags.get(name).and_then(|v| v.last()).map(String::as_str)
    }
    pub fn has_flag(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }
    pub fn bool_flag(&self, name: &str) -> Result<Option<bool>, CliFailure> {
        let Some(vals) = self.flags.get(name) else { return Ok(None); };
        let Some(v) = vals.last() else { return Ok(Some(true)); };
        match v.to_ascii_lowercase().as_str() {
            "" | "true" | "1" | "yes" => Ok(Some(true)),
            "false" | "0" | "no" => Ok(Some(false)),
            other => Err(CliFailure::usage(format!("Invalid boolean for --{name}: '{other}' (expected true|false)."))),
        }
    }
    pub fn req_flag(&self, name: &str) -> Result<&str, CliFailure> {
        self.flag(name).ok_or_else(|| CliFailure::usage(format!("Missing required flag --{name} for `{}`.", self.command_path)))
    }
    pub fn pos(&self, idx: usize, label: &str) -> Result<&str, CliFailure> {
        self.positionals.get(idx).map(String::as_str).ok_or_else(|| {
            CliFailure::usage(format!("Missing positional argument {label} for `{}`.", self.command_path))
        })
    }
}

pub enum ParseOutcome {
    Help(String),
    Version(String),
    Run(ParsedCmd),
}

pub fn parse(raw: &[String]) -> Result<(GlobalOpts, ParseOutcome), (GlobalOpts, CliFailure)> {
    let mut opts = GlobalOpts::default();
    let mut rest: Vec<String> = vec![];
    let mut i = 0;
    let mut help_wanted = false;
    let mut version_wanted = false;
    let mut deferred_flags: Vec<(String, String)> = vec![];
    while i < raw.len() {
        let a = &raw[i];
        if a == "--" {
            rest.extend_from_slice(&raw[i + 1..]);
            break;
        }
        match a.as_str() {
            "-h" | "--help" => { help_wanted = true; i += 1; continue; }
            "-V" | "--version" => { version_wanted = true; i += 1; continue; }
            "--json" => { opts.json = true; i += 1; continue; }
            "--dry-run" => { opts.dry_run = true; i += 1; continue; }
            "--quiet" | "-q" => { opts.quiet = true; i += 1; continue; }
            "--strict" => {
                if let Some(next) = raw.get(i + 1).filter(|n| matches!(n.to_ascii_lowercase().as_str(), "true" | "false")) {
                    deferred_flags.push(("strict".into(), next.clone()));
                    i += 2;
                } else {
                    deferred_flags.push(("strict".into(), "true".into()));
                    i += 1;
                }
                continue;
            }
            _ if a.starts_with("--strict=") => {
                deferred_flags.push(("strict".into(), a["--strict=".len()..].to_string()));
                i += 1;
                continue;
            }
            "--lib" => {
                let Some(val) = raw.get(i + 1) else {
                    return Err((opts, CliFailure::usage("--lib requires a directory path.")));
                };
                opts.lib = Some(PathBuf::from(val));
                i += 2;
                continue;
            }
            _ if a.starts_with("--lib=") => {
                opts.lib = Some(PathBuf::from(&a["--lib=".len()..]));
                i += 1;
                continue;
            }
            _ => {
                rest.push(a.clone());
                i += 1;
            }
        }
    }

    if version_wanted {
        return Ok((opts, ParseOutcome::Version(format!("qmd {} (fdl_core {})", env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_VERSION")))));
    }
    if rest.first().is_some_and(|s| s == "cli") {
        rest.remove(0);
    }
    if rest.first().is_some_and(|s| s == "help") {
        help_wanted = true;
        rest.remove(0);
    }
    if help_wanted || rest.is_empty() {
        return Ok((opts, ParseOutcome::Help(format_help(&rest))));
    }

    // Match longest command path in CATALOG (`group sub` or `group`).
    let two = if rest.len() >= 2 { Some(format!("{} {}", rest[0], rest[1])) } else { None };
    let (spec, consumed) = if let Some(ref p2) = two && let Some(s) = CATALOG.iter().find(|c| c.path == p2) {
        (s, 2usize)
    } else if let Some(s) = CATALOG.iter().find(|c| c.path == rest[0]) {
        (s, 1usize)
    } else if TOP_GROUPS.iter().any(|(g, _)| *g == rest[0]) {
        let sub_hint = CATALOG.iter().filter(|c| c.path.starts_with(&format!("{} ", rest[0]))).map(|c| c.path).collect::<Vec<_>>();
        let msg = if rest.len() < 2 {
            format!("Subcommand required for `{}`. Available: {}", rest[0], sub_hint.join(", "))
        } else {
            format!("Unknown subcommand `{} {}`. Available: {}", rest[0], rest[1], sub_hint.join(", "))
        };
        return Err((opts, CliFailure::usage(msg).with_valid(sub_hint)));
    } else {
        return Err((opts, CliFailure::usage(format!("Unknown command `{}`.", rest[0]))
            .with_valid(TOP_GROUPS.iter().map(|(g, _)| *g))
            .with_hint("Run `qmd --help` or `qmd schema commands --json`.")));
    };

    let mut flags: HashMap<String, Vec<String>> = HashMap::new();
    for (k, v) in deferred_flags {
        if !spec.flags.iter().any(|f| f.name == k) {
            let valid = spec.flags.iter().map(|f| format!("--{}", f.name)).collect::<Vec<_>>();
            return Err((opts, CliFailure::usage(format!("Unknown flag `--{k}` for `{}`.", spec.path)).with_valid(valid)));
        }
        flags.entry(k).or_default().push(v);
    }
    let mut positionals: Vec<String> = vec![];
    let mut j = consumed;
    let mut allow_flags = true;
    while j < rest.len() {
        let tok = &rest[j];
        if allow_flags && tok == "--" {
            allow_flags = false;
            j += 1;
            continue;
        }
        let is_css_var_pos = spec.path == "theme set-var" && positionals.len() == 1;
        if allow_flags && !is_css_var_pos && let Some(long) = tok.strip_prefix("--") {
            let (name, inline_val) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let Some(fspec) = spec.flags.iter().find(|f| f.name == name) else {
                let valid = spec.flags.iter().map(|f| format!("--{}", f.name)).collect::<Vec<_>>();
                return Err((opts, CliFailure::usage(format!("Unknown flag `--{name}` for `{}`.", spec.path)).with_valid(valid)));
            };
            match (fspec.value, inline_val) {
                (None, Some(v)) => { flags.entry(name.into()).or_default().push(v); j += 1; }
                (None, None) => {
                    if let Some(next) = rest.get(j + 1).filter(|n| matches!(n.to_ascii_lowercase().as_str(), "true" | "false")) {
                        flags.entry(name.into()).or_default().push(next.clone());
                        j += 2;
                    } else {
                        flags.entry(name.into()).or_default().push("true".into());
                        j += 1;
                    }
                }
                (Some(_), Some(v)) => { flags.entry(name.into()).or_default().push(v); j += 1; }
                (Some(_), None) => {
                    let Some(next) = rest.get(j + 1) else {
                        return Err((opts, CliFailure::usage(format!("Flag `--{name}` requires a value."))));
                    };
                    flags.entry(name.into()).or_default().push(next.clone());
                    j += 2;
                }
            }
        } else {
            positionals.push(tok.clone());
            j += 1;
        }
    }

    for f in spec.flags {
        if f.required && !flags.contains_key(f.name) {
            return Err((opts, CliFailure::usage(format!("Missing required flag --{} for `{}`.", f.name, spec.path))
                .with_hint(format!("Run `qmd {} --help` for usage.", spec.path))));
        }
    }

    Ok((opts, ParseOutcome::Run(ParsedCmd { command_path: spec.path.into(), positionals, flags, spec })))
}

pub fn format_help(topic: &[String]) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    if topic.len() >= 2 {
        let p2 = format!("{} {}", topic[0], topic[1]);
        if let Some(c) = CATALOG.iter().find(|c| c.path == p2) {
            return format_cmd_help(c);
        }
    }
    if let Some(t) = topic.first() {
        if let Some(c) = CATALOG.iter().find(|c| c.path == t) {
            return format_cmd_help(c);
        }
        let matches: Vec<&CmdSpec> = CATALOG.iter().filter(|c| c.path.starts_with(&format!("{t} "))).collect();
        if !matches.is_empty() {
            let _ = writeln!(s, "qmd {t} — subcommands:\n");
            for c in matches {
                let _ = writeln!(s, "  {:<24} {}", c.path, c.summary);
            }
            let _ = writeln!(s, "\nRun `qmd <command> --help` for detailed flags & examples.");
            return s;
        }
    }
    let _ = writeln!(s, "qmd {} — Fine-grained CLI for Quiet.md rich documents, boards and themes\n", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(s, "USAGE:\n  qmd [GLOBAL FLAGS] <GROUP> [SUBCOMMAND] [ARGS...]\n");
    let _ = writeln!(s, "GLOBAL FLAGS:");
    let _ = writeln!(s, "  --lib <PATH>             Target Quiet.md library folder (overrides QUIET_MD_HOME)");
    let _ = writeln!(s, "  --json                   Structured JSON output (recommended for AI agents)");
    let _ = writeln!(s, "  --dry-run                Validate and preview changes without writing to disk");
    let _ = writeln!(s, "  --quiet, -q              Suppress stdout on success");
    let _ = writeln!(s, "  -h, --help               Print help\n");
    let _ = writeln!(s, "COMMAND GROUPS:");
    for (g, desc) in TOP_GROUPS {
        let _ = writeln!(s, "  {:<14} {}", g, desc);
    }
    let _ = writeln!(s, "\nAGENT QUICK START:");
    let _ = writeln!(s, "  qmd schema doc-spec --json       Inspect full DocSpec JSON schema & valid options");
    let _ = writeln!(s, "  qmd schema palette --json        List all DocxXML font/highlight/callout/cell colors");
    let _ = writeln!(s, "  qmd library tree --json          List folders and documents with ids & paths");
    let _ = writeln!(s, "  qmd <group> --help               Show subcommands of a group");
    s
}

fn format_cmd_help(c: &CmdSpec) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(s, "{}", c.summary);
    let _ = write!(s, "\nUSAGE:\n  qmd {}", c.path);
    for p in c.positionals { let _ = write!(s, " {p}"); }
    if !c.flags.is_empty() { s.push_str(" [FLAGS]"); }
    s.push('\n');
    if !c.flags.is_empty() {
        s.push_str("\nFLAGS:\n");
        for f in c.flags {
            let lhs = match f.value { Some(v) => format!("--{} <{v}>", f.name), None => format!("--{}", f.name) };
            let req_tag = if f.required { " (required)" } else { "" };
            let _ = writeln!(s, "  {:<28} {}{}", lhs, f.desc, req_tag);
        }
    }
    if !c.examples.is_empty() {
        s.push_str("\nEXAMPLES:\n");
        for ex in c.examples { let _ = writeln!(s, "  {ex}"); }
    }
    s
}
