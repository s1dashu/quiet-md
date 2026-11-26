# quiet: the Quiet.md library for agents

`quiet` reads, searches and edits a Quiet.md library: a folder of documents
(`.xml` DocxXML and `.md` Markdown), boards (`.excalidraw`) and 数据表
(`.sqlite`). `quiet guide` prints this guide as the installed `quiet` knows
it; `quiet help COMMAND` shows one command in detail.

The library is plain files, and you may read them directly. Edit through
`quiet` instead: it keeps the structure valid, rewrites links when a document
moves, keeps the replaced text as a version, and refuses to overwrite a
change made since you read. A person may have the same document open in the
app; a clean editor there takes in what you wrote.

## Two ways in

- **Commands** for one step: `quiet outline 周报`, `quiet insert 周报 --after '#3~k7f2' --md '...'`.
- **`quiet run`** for anything with a loop, a filter or several steps: a short
  JavaScript script against `lib`. Intermediate results stay in the script;
  only what you print or return comes back. The whole script is one
  transaction: if it throws, nothing is written.

Prefer one `run` over many commands when the next step does not need your
judgement on the previous result.

## Addressing

- A document is its path (`工作/周报.xml`; the extension may be left off) or its title.
- A block is a **ref** as `outline`, `blocks` and `find` print it: `#3~k7f2`,
  its 1-based position (`#3.2` is the second child of `#3`) and a
  fingerprint of its text. A ref checks itself: if the document changed and
  the block moved, it is still found; if the block itself changed or is
  gone, the command fails with `stale_ref` instead of touching another block.
  So you can use refs from an earlier read; on `stale_ref`, read again.
- On the command line the block may also be the next argument: `quiet delete 周报 风险 --section`.
- A bare position (`#3`) or a heading's text (`'本周进展'`) also works, unchecked;
  a text that several headings contain fails with `ambiguous_ref`, listing their refs.
- `--section` (or `{section: true}`) extends a heading to its section: the
  heading and everything after it up to the next heading at its level or above.
  `--no-heading` (`{section: true, heading: false}`) is the section without its heading.
- `--end-of HEADING` (`{endOf: ref}`) places new or moved blocks at the end of
  that heading's section, e.g. a new item at the end of a section's list.
- Each read prints the document's **rev**. `--expect-rev REV` on a write fails
  with `stale_rev` if the document changed at all since.
- Inside one `quiet run`, every block also has an **id** (`b17`) that stays put
  however the script edits the document. Block objects carry both.

## Commands

```
quiet ls [FOLDER] [-r] [--kind doc|md|board|table|folder]
quiet find WORDS… [--in FOLDER] [--limit N]    documents holding every word, best first, with matching blocks
quiet outline DOC                              headings with refs and section sizes; rev
quiet read DOC[#REF] [--section|--no-heading] [--as md|text|xml|json] [--max-chars N]   the body (the title is the document's name); 40000 chars by default, 0: all
quiet blocks DOC[#REF] [--kind K] [--grep TEXT] [--done|--undone]
quiet links DOC                                @mentions out of DOC and into it
quiet check                                    unreadable files, broken links, empty pages, unused assets

quiet new TITLE [--in FOLDER] [--md TEXT|-] [--kind doc|md|board|table] [--fields A,B | --csv TEXT|-]
quiet mkdir NAME [--in FOLDER]
quiet insert DOC [--after|--before|--into|--end-of REF | --start | --end] --md TEXT|-
quiet replace DOC#REF [--section] --md TEXT|-
quiet delete DOC#REF [--section]
quiet move DOC#REF --after|--before|--into|--end-of REF | --start | --end [--section]
quiet set DOC#REF [--text TEXT] [--done|--undone] [--kind text|h1..h6|bullet|ordered|todo|quote|code]
quiet rename DOC TITLE
quiet mv DOC FOLDER          (within the document's top folder; its name alone: its top)
quiet rm DOC                 to the trash (kept 30 days)

quiet table DOC[#REF] [--table T] [--view V] [--where 'FIELD OP VALUE']… [--any] [--sort F[:desc]]… [--group F]…
                 [--fields A,B] [--stats [sum,…]] [--as json|csv|md] [--limit N] [--offset N]
quiet table add DOC[#REF] --set Field=Value … | --rows JSON|-
quiet table update DOC[#REF] ROW --set Field=Value … | --rows JSON|-   (or --where COND… --set …)
quiet table delete DOC[#REF] ROW… | --where COND…
quiet table import DOC (--csv FILE|- | --rows JSON|-) [--create-fields]
quiet table field add DOC NAME --type T [--options A,B] [--formula EXPR] [--link TABLE] [--lookup LINK.FIELD] [--format F] [--after F]
quiet table field change DOC NAME [--name N] [--type T] [--add-option X] [--remove-option X] [--rename-option OLD=NEW] …
quiet table field rename DOC NAME NEW | quiet table field delete DOC NAME…
quiet table view DOC | view add DOC NAME [--kind K] | view set DOC NAME … | view rename DOC NAME NEW | view delete DOC NAME
quiet table new DOC NAME [--fields A,B] | quiet table rename DOC NAME NEW | quiet table remove DOC NAME [--force]
quiet board DOC[#REF] [--full]
quiet board add DOC[#REF] --elements JSON|- [--replace]
quiet board rm DOC[#REF] ID…

quiet history DOC [VERSION]  versions kept (1 = newest), or what changed since one
quiet restore DOC VERSION    put DOC back as it was (undoable: the current text is kept first)
quiet trash                  what is in the trash
quiet untrash NAME           bring a document back
quiet import FILE… [--in FOLDER]   .md/.txt, .xml, .csv/.tsv (as a 数据表), .excalidraw

quiet run [FILE|-] [-e CODE] [--timeout SECS]
quiet call OP [JSON]         any operation by name, as run's lib calls it
```

Every write takes `--dry-run` (print what would change, write nothing) and
`--expect-rev REV`. New content is Markdown: headings, lists, `- [ ]` todos,
quotes, code fences, tables, links, **bold** and so on; `--md -` reads stdin.
**Links to other documents** (@mentions, which follow a document when it
moves and show as backlinks) are written `[[path or title]]`, e.g.
`见 [[项目/方案]]`; a Markdown link to a library file (`[方案](项目/方案.xml)`)
becomes one too. `[[…]]` looks in the document's own top folder first (a
new document's: the one it lands in), then in the others; a name only one
other document has is that one, and several are `ambiguous` (the error lists
them by path and id: write the id, `[[id]]`, or the full path).
Reading as Markdown writes mentions back as `[[path]]`, or as `[[id]]` when
the path would find another document from here, so copied sections keep
their links.
**Where the library is.** Every folder the user has open in Quiet.md on
this computer: its top folders. Paths start with one of them: the
first part is the top folder's name (any case), e.g. `Quiet.md/工作/周报`;
`quiet ls` lists the top folders, where each is on disk, and why one is
unavailable. Every path the CLI prints is such a path. A path or title only
one document has works without the top folder; one that several have is
`ambiguous`, listing them by path: pass the full path, or the id from
`ls --json`. Two top folders of one name: the error lists them with their
keys; a key works as the first part instead of the name. New documents,
folders and imports go into the `--in` folder, else the first top folder,
and the output's path says where. Each top folder keeps its own trash and
versions; moving a document to another one is not supported
(`cross_folder`). Global options: `--library DIR` (that folder on its own,
paths starting in it, not the app's list) and `--json` (every field, for
parsing).

Kinds of blocks: text, h1…h9, bullet, ordered, todo, code, quote, callout,
divider, image, table, grid, column, file, board, datatable, countdown,
subpages, raw:TAG (kept verbatim).

**数据表.** Rows have lasting ids (`r12`). Values are text as the app shows
them and are written as if pasted: numbers, dates, `true`/`false` for
checkboxes, select options (missing ones are added), several options joined
with commas, a link (关联) as the other table's first-field text or `r12`.
Computed fields are read-only. `table` prints the file's tables and views and
each field's type, options, formula, link or lookup (`--json`: `schema`).
A new table: `quiet new 书单 --kind table --fields 书名,作者,状态`, or from CSV
(types guessed from the values) with `--csv` or `quiet import file.csv`.

Read what you need, not the whole table: `--view V` shows a view as the app
does (its 筛选, 排序, 分组, hidden fields); `--where 'FIELD OP VALUE'`
(repeatable, all must hold; `--any`: any of them) adds conditions on top.
OP: `= != ~ !~ > >= < <= empty notempty before after` (or 等于 包含 为空 早于…);
a field takes only some (the error lists them). Dates compare with a day or
`today`, `this_week`, `past_7`, `this_month`… (今天, 本周, 过去 7 天内). Selects
compare by option name (`~` any of `a,b`). `--sort F:desc`, `--group F`,
`--fields A,B`, `--stats` (each column's 统计; or `--stats sum,average`),
`--as csv|md` (the rows as CSV or a Markdown table). Change many rows at
once with `table update DOC --where … --set F=V` or `table delete DOC --where …`
(check with `--dry-run`: it lists the rows).

Fields, views and tables change like in the app. Types by key or Chinese
name: text single_select multi_select person date attachment number checkbox
url formula lookup auto_number phone email location barcode progress
currency rating link created_time modified_time. A type change converts the
cells and reports how many it lost; a formula is checked first (`bad_formula`
says where). The first field, the last view and the last table stay. `quiet
help table` has every option.

**画板.** `board` lists elements: id, type, x,y, w×h, text or label, and
`from → to` for bound arrows, plus the bounds of the whole drawing. Add
elements with Excalidraw skeletons (as `convertToExcalidrawElements`):
`{"type": "rectangle"|"ellipse"|"diamond"|"text"|"arrow"|"line"|"frame",
"x", "y", "width", "height", "label": {"text"}, "text" (text elements),
"id" (your own, to point arrows at), "start": {"id"}, "end": {"id"} (arrows),
"strokeColor", "backgroundColor"}`. An arrow with `start` and `end` ids and no
x/y is drawn from edge to edge and bound to both (ids may name elements
already on the board). `added` names the new shapes, then the arrows; the
text inside a shape is its label, listed with it. Place new shapes beside
the current bounds.
A new board: `quiet new 架构图 --kind board`.

## Script API

Write the script to a file and run `quiet run script.js`; short ones go in
`quiet run -e '…'`, and `quiet run -` reads it from stdin.
Top-level `await` and `return` work; a returned value is printed. `help()` and
`help("name")` print this section from inside a script.

- `lib.ls(folder?, {recursive, kind})` → `[{path, id, title, kind, modified}]` (no folder: the top folders, `{path, top, dir, key, available, documents?, error?, default?}`)
- `lib.find(words, {folder, limit})` → `{total, results: [{path, title, rev, hits: [{ref, kind, text}]}]}`
- `lib.doc(path, {rev})` → `Doc` (throws `not_found` / `ambiguous` / `stale_rev`)
- `lib.create(title, {folder, md, kind, fields, csv})` → `Doc` (a table: `{path}`), written when the script ends
- `lib.mkdir(name, {folder})`
- `lib.check()` → `{documents, issues: [{path, code, message}]}`
- `lib.trash()` → `[{id, title, folder, deleted}]`; `lib.untrash(name)`
- `lib.call(op, args)` → any operation, as `quiet call`
- `doc.path`, `doc.title`, `doc.kind`, `doc.rev`, `doc.block_count`
- `doc.outline()` → `{headings: [{ref, level, text, chars}], top?, chars, …}`
- `doc.blocks({kind, grep, done, within})` → `[{ref, id, kind, text, depth, level?, done?, children?}]`
- `doc.block(ref)`, `doc.section(ref)` → block objects
- `doc.markdown(ref?, {section, heading})`, `doc.text(ref?, {section, heading})` → string (whole, uncut; `heading: false` leaves the section's heading out)
- `doc.insert(md, where)` → `[{ref, id}]`; where: `"start"`, `"end"` (default), `{after: ref}`, `{before: ref}`, `{into: ref}` (as last child), `{endOf: heading}` (end of its section)
- `doc.append(md)`, `doc.prepend(md)`
- `doc.replace(ref, md, {section})`, `doc.remove(ref, {section})`, `doc.move(ref, where, {section})`
- `doc.setText(ref, text)` (inline Markdown), `doc.setDone(ref, done = true)`, `doc.setKind(ref, kind)`
- `doc.rename(title)`, `doc.moveTo(folder)`, `doc.trash()` — applied when the script ends
- `doc.links()` → `{out: [{to, broken, text}], back: [{from, title, text}]}` (out: as edited in this script; back: as on disk)
- `doc.table({at, table, view, where, any, sort, group, fields, stats, as, limit, offset})` → `{fields, schema, views, tables, total, rows: [{_id, field: text}], groups?, stats?, text?}` (every value is a string: `Number(r.评分)` to compare; `where`: `"状态 = 在读"`, `{field, op, value}` or a list; `sort`/`group`: `"F:desc"` or a list; `as: "csv"|"md"` gives `text`)
- `doc.addRows([{field: value}], {at, table})` → `["r13", …]`; `doc.updateRow(row, {field: value})`; `doc.updateRows([{_id, field: value}])`; `doc.deleteRows(rows)`
- `doc.updateWhere(where, {field: value}, {any, view})` → ids; `doc.deleteWhere(where)` → `["r3 三体", …]`; `doc.importRows([{field: value}] or csvText, {createFields})`
- `doc.addField(name, type, {options, formula, link, lookup, rollup, format, after, twoWay})`; `doc.changeField(name, {name, type, options, addOptions, removeOptions, renameOptions: {old: new}, formula, format})` → `{converted, lost, lost_rows}`; `doc.deleteFields(names)`
- `doc.views()` → `[{name, kind, where, sort, group, hidden, stats, rows}]`; `doc.addView(name, {kind})`; `doc.setView(name, {where, any, sort, group, hide, show, order, stack, date, end, stats: {field: stat}, clear})`; `doc.renameView(name, to)`; `doc.deleteView(name)`
- `doc.addTable(name, {fields, csv})`; `doc.renameTable(name, to)`; `doc.removeTable(name, {force})` (each table op takes `{table}` to pick a table of the file)
- `doc.board({at, full})` → `{elements, bounds}`; `doc.addToBoard(skeletons, {at, replace})` → ids; `doc.removeFromBoard(ids)`
- `doc.history(version?)` → `{versions}` or `{diff}`; `doc.restore(version)`
- `console.log(...)` / `print(...)`: objects print as JSON

Wherever a ref is taken, a block object or its id works too; wherever a row
is taken, a row object works too. Errors thrown by `lib` carry `code` and
`hint`; catch them to recover inside the script.

The script runs in a sandbox: no files, network or processes, only `lib`
(so no `import`; run `quiet import` from the shell). It stops after 60 s
(`--timeout`). A document or board made with `lib.create` can be read and
edited in the same script; it is written when the script ends (so it cannot
be mentioned with `[[…]]` until the next run). A new 数据表 can be filled from
the next run. Renames, moves and trashing also take effect when the script
ends, so later reads in the same script see the old paths.

### Examples

Collect the open todos of every meeting note into one page:

```js
const notes = lib.ls("会议", { recursive: true }).filter(e => e.kind === "doc" || e.kind === "md");
let md = "";
for (const e of notes) {
  const open = lib.doc(e.path).blocks({ kind: "todo", done: false });
  if (open.length) md += `## ${e.title}\n` + open.map(t => `- [ ] ${t.text}`).join("\n") + "\n\n";
}
return lib.create("待办汇总", { md }).path;
```

Split a long document into one page per second-level section (the heading
becomes the title, so it is left out of the body); link them from the
original in a second run, once they exist:

```js
const doc = lib.doc("方案");
lib.mkdir("方案拆分");
for (const h of doc.outline().headings.filter(h => h.level === 2)) {
  lib.create(h.text, { folder: "方案拆分", md: doc.markdown(h.ref, { section: true, heading: false }) });
}
```

```sh
quiet insert 方案 --end --md $'## 拆分后的文档\n- [[方案拆分/背景]]\n- [[方案拆分/目标]]'
```

Check off every todo that mentions a shipped feature, then show what is left:

```js
const doc = lib.doc("发布计划");
for (const t of doc.blocks({ kind: "todo", done: false, grep: "导出" })) doc.setDone(t);
return doc.blocks({ kind: "todo", done: false }).map(t => t.text);
```

Mark finished books in a 数据表 and count what is left:

```js
const books = lib.doc("书单");
books.updateWhere("进度 = 100%", { 状态: "读完" });
return books.table({ where: "状态 != 读完", limit: 0 }).total;
```

Give a 数据表 a 看板 of what is being read, with a score column:

```sh
quiet table field add 书单 评分 --type rating --format stars=5
quiet table view add 书单 在读 --kind kanban
quiet table view set 书单 在读 --where '状态 != 读完' --sort 评分:desc --stack 状态
quiet table 书单 --view 在读 --stats --as md
```

Draw a flow on a new board from a document's headings (an arrow with
`start`/`end` ids and no position runs from edge to edge, bound to both):

```js
const steps = lib.doc("流程").outline().headings.filter(h => h.level === 2);
const board = lib.create("流程图", { kind: "board" });
board.addToBoard(steps.flatMap((h, i) => [
  { type: "rectangle", id: `s${i}`, x: i * 260, y: 0, width: 200, height: 80, label: { text: h.text } },
  ...(i ? [{ type: "arrow", start: { id: `s${i - 1}` }, end: { id: `s${i}` } }] : []),
]));
```

## Working well

- Look before you edit: `outline` for structure, `read DOC#REF --section` for a part.
- Long documents: `outline` first, then read only the sections you need.
- Use `--dry-run` on a large change and read the diff before writing it.
- Made a mistake? `quiet history DOC` and `quiet restore DOC 1` undo your last save.
- Consecutive list items read as one list. To add a separate list after
  another (at the end of a document, say), start it with a heading or a line of text.
- In a script, print a small result (counts, paths, refs), not whole documents.
- Exit status: 0 ok, 2 usage, 3 not found / stale or bad ref, 4 stale rev or changed on disk, 1 other.
  Errors print `error[code]: message` and a `hint:` line with what to do next.
- If the app saved the document while you worked, your write fails with
  `stale_rev`, `stale_ref` or `conflict`; read again and redo it.
