//! `qmd schema` — machine-readable discovery for AI agents.

use crate::args::{CATALOG, ParsedCmd};
use crate::assets;
use crate::output::{GlobalOpts, print_success};
use fdl_core::{lang, model::COUNTDOWN_COLORS, palette, theme_css};
use serde_json::{Value, json};

fn palette_json() -> Value {
    let font: Vec<Value> = palette::FONT_COLORS.iter().filter_map(|(n, hex)| {
        let name = (*n)?;
        Some(json!({ "name": name, "hex": format!("#{hex:06x}") }))
    }).collect();
    let highlight: Vec<Value> = palette::BG_COLORS.iter().flatten().map(|n| {
        let rgba = palette::bg_color(n).unwrap();
        json!({ "name": n, "css": theme_css::rgba_css(rgba) })
    }).collect();
    let cell_bg: Vec<Value> = palette::CELL_BG_COLORS.iter().flatten().map(|n| {
        let rgba = palette::block_bg(n).unwrap();
        json!({ "name": n, "css": theme_css::rgba_css(rgba) })
    }).collect();
    let callout_bg: Vec<Value> = palette::CALLOUT_BG_COLORS.iter().flatten().map(|n| {
        let rgba = palette::callout_bg(n).unwrap();
        json!({ "name": n, "css": theme_css::rgba_css(rgba) })
    }).collect();
    let callout_border: Vec<Value> = palette::HUES.iter().map(|n| {
        let rgba = palette::callout_border(n).unwrap();
        json!({ "name": n, "css": theme_css::rgba_css(rgba) })
    }).collect();
    json!({
        "font_colors": font,
        "text_highlights": highlight,
        "block_and_cell_backgrounds": cell_bg,
        "callout_backgrounds": callout_bg,
        "callout_borders": callout_border,
        "countdown_colors": COUNTDOWN_COLORS,
    })
}

fn langs_json() -> Value {
    let list: Vec<Value> = lang::LANGS.iter().map(|(name, token, aliases)| {
        json!({ "name": name, "token": token, "aliases": aliases })
    }).collect();
    json!({ "languages": list })
}

fn covers_json() -> Value {
    let series: Vec<Value> = assets::COVER_SERIES.iter().map(|(prefix, label, count)| {
        let names: Vec<String> = (1..=*count).map(|i| format!("{prefix}-{i}")).collect();
        json!({ "series": prefix, "label": label, "covers": names })
    }).collect();
    json!({ "series": series, "all": assets::cover_names() })
}

fn doc_spec_json() -> Value {
    json!({
        "description": "Declarative JSON document specification accepted by `qmd doc create --spec` and `qmd doc apply --spec` (and output by `qmd doc get --json`).",
        "top_level": {
            "title": "string",
            "title_runs": "optional RunSpec[]",
            "icon": "optional emoji literal or emoji-mart id (e.g. 'rocket', '📘')",
            "cover": { "gallery": "built-in cover name", "file": "local path", "x": "f32 (-1..1)", "y": "f32 (-1..1)" },
            "meta": {
                "width": "standard (820px) | wide (1020px) | full",
                "pinned": "bool",
                "starred": "bool",
                "reading": "bool",
                "catalogue_open": "bool",
                "refs_hidden": "bool",
                "info_hidden": "bool",
                "info_created": "bool",
                "info_modified": "bool"
            },
            "blocks": "BlockSpec[]"
        },
        "block_types": {
            "p": "{ type:'p', text?, runs?, align?, bg?, folded?, children? }",
            "h1..h9": "{ type:'h1'..'h9'|'heading', level?, seq_auto?, text?, runs?, align?, bg?, folded?, children? }",
            "bullet": "{ type:'bullet', text?, runs?, align?, bg?, folded?, children? }",
            "ordered": "{ type:'ordered', seq?, text?, runs?, align?, bg?, folded?, children? }",
            "todo": "{ type:'todo', done?, text?, runs?, align?, bg?, folded?, children? }",
            "code": "{ type:'code', lang, caption?, wrap?, text }",
            "quote": "{ type:'quote', bg?, children }",
            "callout": "{ type:'callout', emoji?, bg?, border?, text_color?, children }",
            "divider": "{ type:'divider' }",
            "image": "{ type:'image', src, width?, height?, caption?, align?, crop?:[l,t,r,b] }",
            "file": "{ type:'file', src, name?, view?:'card'|'preview', align? }",
            "grid": "{ type:'grid', columns:[{ ratio?:f32, children:BlockSpec[] }] } (2..=10 columns, normalized sum=1.0, each >= 1/24)",
            "table": "{ type:'table', col_widths?:f32[], header_rows?:usize, header_col?:bool, rows:TableCellSpec[][] }",
            "countdown": "{ type:'countdown', countdown:{ date?:'YYYY-MM-DD', time?:'HH:MM', duration?:f64, color?:'#FF8800', notify?:bool, dhms?:[d,h,m,s] } }",
            "sub_page_list": "{ type:'sub_page_list' }",
            "board": "{ type:'board', height?:f32, diagram?:DiagramSpec, scene?:ExcalidrawSceneJSON }",
            "raw": "{ type:'raw', xml }"
        },
        "run_spec": {
            "text_run": "{ text, bold?, italic?, underline?, strike?, code?, link?, text_color?, bg_color? }",
            "inline_equation": "{ equation: 'E=mc^2' }",
            "inline_button": "{ button: { text, action:'OpenLink'|'DuplicatePage'|'FollowPage', src?, color? } }",
            "inline_reminder": "{ reminder: { date:'YYYY-MM-DD', time?:'HH:MM', should_notify?:true } }",
            "inline_doc_link": "{ doc_link: { target:'id|rel|title', title? } }",
            "inline_file": "{ file: { src, name? } }"
        }
    })
}

fn board_spec_json() -> Value {
    json!({
        "description": "Semantic diagram specification accepted by `qmd board build`, `qmd board create`, `qmd board insert-into-doc`, and `board.diagram` inside DocSpec.",
        "fields": {
            "layout": "flow-lr | flow-tb | grid | manual",
            "theme": "craft-pastel | slate | warm | contrast",
            "background": "optional CSS hex background",
            "height": "f32 page block height in px (default 400)",
            "gap_x": "f64 horizontal gap (default 90)",
            "gap_y": "f64 vertical gap (default 56)",
            "cols": "usize grid column count (default 3)",
            "nodes": "[{ id, shape:'rectangle'|'diamond'|'ellipse'|'sticky'|'text', label, x?, y?, width?, height?, fill?, stroke?, stroke_width?, stroke_style?:'solid'|'dashed'|'dotted', fill_style?:'solid'|'hachure'|'cross-hatch', rounded?:true, roughness?:0, font_size?, text_align? }]",
            "edges": "[{ from, to, label?, style?:'elbow'|'straight'|'curved', stroke?, stroke_style?, start_arrow?, end_arrow?:'arrow' }]",
            "frames": "[{ name, includes:[node_id...], padding?:28 }]"
        }
    })
}

fn themes_json() -> Value {
    let props: Vec<Value> = theme_css::PROPS.iter().map(|p| {
        json!({ "sel": p.sel, "prop": p.prop, "key": p.key, "kind": format!("{:?}", p.kind), "feishu_used": p.feishu_used })
    }).collect();
    let bundled: Vec<&str> = theme_css::BUNDLED.iter().map(|(id, _)| *id).collect();
    json!({ "bundled_themes": bundled, "tokens": theme_css::TOKEN_NAMES, "props": props })
}

pub fn run(cmd: &ParsedCmd, opts: &GlobalOpts) -> Result<(), crate::output::CliFailure> {
    let topic = cmd.positionals.first().map(String::as_str).unwrap_or("all");
    let data = match topic {
        "palette" => palette_json(),
        "langs" | "languages" => langs_json(),
        "covers" => covers_json(),
        "doc-spec" | "doc" => doc_spec_json(),
        "board-spec" | "board" => board_spec_json(),
        "themes" => themes_json(),
        "commands" => serde_json::to_value(CATALOG).unwrap(),
        "all" => json!({
            "commands": CATALOG,
            "doc_spec": doc_spec_json(),
            "board_spec": board_spec_json(),
            "palette": palette_json(),
            "langs": langs_json(),
            "covers": covers_json(),
            "themes": themes_json(),
        }),
        other => {
            return Err(crate::output::CliFailure::new("UNKNOWN_SCHEMA_TOPIC", format!("Unknown schema topic '{other}'."))
                .with_valid(["all", "commands", "doc-spec", "board-spec", "palette", "langs", "covers", "themes"]));
        }
    };
    let human = if opts.json { String::new() } else { serde_json::to_string_pretty(&data).unwrap_or_default() };
    print_success(opts, "schema", None, data, &[], &[], &human);
    Ok(())
}
