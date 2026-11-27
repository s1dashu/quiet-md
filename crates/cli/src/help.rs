//! `quiet help` and `quiet help COMMAND`: the overview, and each command
//! with its options and an example, so an agent need not read the whole
//! guide (`quiet guide`) to call one.

use crate::fail::{R, fail};

pub const HELP: &str = "quiet — read, search, edit and script a Quiet.md library

  quiet ls [FOLDER] [-r] [--kind doc|md|board|table|folder]
  quiet find WORDS… [--in FOLDER] [--limit N]
  quiet outline DOC                      headings with refs and sizes
  quiet read DOC[#REF] [--section] [--as md|text|xml|json] [--max-chars N]
  quiet blocks DOC[#REF] [--kind K] [--grep TEXT] [--done|--undone]
  quiet links DOC                        mentions out of and into DOC
  quiet check                            broken links, unreadable files, unused assets

  quiet new TITLE [--in FOLDER] [--md TEXT|-] [--kind doc|md|board|table] [--open]
  quiet mkdir NAME [--in FOLDER]
  quiet insert DOC [--after|--before|--into|--end-of REF | --start | --end] --md TEXT|-
  quiet replace DOC#REF [--section] --md TEXT|-
  quiet delete DOC#REF [--section]
  quiet move DOC#REF --after|--before|--into REF [--section]
  quiet set DOC#REF [--text TEXT] [--done|--undone] [--kind K]
  quiet rename DOC TITLE | quiet mv DOC FOLDER | quiet rm DOC
  quiet duplicate DOC [TITLE]            a copy beside it (\"TITLE 副本\")
  quiet pin|unpin DOC                    the sidebar's 置顶 list

  quiet table DOC[#REF] [--view V] [--where 'F OP V']…   a 数据库's rows, filtered
  quiet table add|update|delete|import …  rows;  table field|view|new|rename|remove … its shape
  quiet board DOC[#REF]                  a 画板's elements; board add|rm …
  quiet history DOC [VERSION]            saved versions, or what changed since one
  quiet restore DOC VERSION              put a document back as it was
  quiet trash | quiet untrash NAME       the trash, and bringing a document back
  quiet import FILE… [--in FOLDER]       Markdown, DocxXML, CSV, .excalidraw

  quiet folders [add PATH | remove NAME] the folders Quiet.md's sidebar lists
  quiet open DOC[#REF] | --folder F | --trash   show it in Quiet.md, at that block

  quiet run [FILE|-] [-e CODE]           a JavaScript script, as one transaction
  quiet call OP [JSON]                   any operation by name (what run's lib calls)
  quiet mcp                              serve these as MCP tools on stdio
  quiet guide                            the full guide for agents (script API included)
  quiet help COMMAND                     one command in detail
  quiet telemetry [on|off|status]        the anonymous daily usage counts

Writes take --dry-run (show the diff, write nothing) and --expect-rev REV.
Paths start with a top-level folder: the first part is one of the folders Quiet.md
has open on this computer (`quiet ls` lists them), e.g. Quiet.md/工作/周报.
DOC is such a path (extension optional), a path or title only one document
has, or an id. REF is what outline/blocks print (#3~k7f2: position and
fingerprint), a position (#2.1) or a heading's text. New things go to the
--in folder, else the first folder. Global: --library DIR (that folder on its
own, paths starting in it), --json.

quiet ships inside Quiet.md (https://quiet.md). On macOS, Settings › 连接
Agent › 安装命令行工具 links it into ~/.local/bin (or link
/Applications/quiet.md.app/Contents/MacOS/quiet there yourself); Homebrew's
cask and the Windows installer put it on the PATH; on Linux it is bin/quiet
in the .tar.gz, and the AppImage runs it when called as quiet. The agent
skill: npx skills add s1dashu/quiet-md.";

/// (names, usage and notes).
const COMMANDS: &[(&[&str], &str)] = &[
    (&["ls"], "quiet ls [FOLDER] [-r] [--kind doc|md|board|table|folder]

Lists a folder; with none, the top-level folders: those Quiet.md has open, where each
is on disk and whether it opens (two of one name: use its key, which --json
shows, as the first part of a path). -r goes into subfolders. Each line:
path, [kind], last modified.

  quiet ls 工作 -r --kind doc"),
    (&["find", "search"], "quiet find WORDS… [--in FOLDER] [--limit N]

Documents holding every word (any case) in their title or text, best first,
each with up to 5 matching blocks and their refs. Default limit 20.

  quiet find 季度 预算 --in 工作"),
    (&["outline"], "quiet outline DOC

The document's rev, size and headings, each with its ref and section size;
a document without headings lists its top-level blocks instead. Read this
before reading or editing a long document.

  quiet outline 周报"),
    (&["read", "cat"], "quiet read DOC[#REF] [--section|--no-heading] [--as md|text|xml|json] [--max-chars N]

The document (or the block REF; --section: the heading and its section;
--no-heading: the section without its heading) as
Markdown, plain text, DocxXML or JSON blocks. Prints at most 40000
characters unless --max-chars says otherwise (0: everything).

  quiet read 周报#'本周进展' --section"),
    (&["blocks"], "quiet blocks DOC[#REF] [--kind K] [--grep TEXT] [--done|--undone]

Blocks with ref, kind and text, filtered: --kind text|h1…h9|heading|bullet|
ordered|todo|code|quote|callout|table|image|board|datatable…, --grep (any
case), --done / --undone for todos. With #REF, only that block and its children.

  quiet blocks 发布计划 --kind todo --undone"),
    (&["links"], "quiet links DOC

@mentions of other documents in DOC (broken ones marked) and the documents
that mention DOC."),
    (&["check"], "quiet check

The whole library: files that do not parse, mentions of documents that are
gone, empty documents, pictures and files no document uses."),
    (&["new"], "quiet new TITLE [--in FOLDER] [--md TEXT|-] [--kind doc|md|board|table] [--fields A,B,… | --csv TEXT|-] [--open]

--open shows it in Quiet.md once written, as 新建 there does (`quiet open`).

A new document (DocxXML; --kind md or --markdown for a .md file), its body
from Markdown; an empty board; or a 数据库 with the columns --fields names,
or from CSV text whose first row names them (types guessed from the values).

  quiet new 会议纪要 --in 会议 --md '# 议题\\n- [ ] 预算'
  quiet new 书单 --kind table --fields 书名,作者,状态
  quiet new 架构图 --kind board"),
    (&["mkdir"], "quiet mkdir NAME [--in FOLDER]"),
    (&["insert", "add"], "quiet insert DOC [--after|--before|--into|--end-of REF | --start | --end] --md TEXT|-

New blocks from Markdown; at the end by default. DOC#REF alone means
--after REF. --into puts them inside REF (a list item, quote, callout,
column) after its last child; --end-of at the end of heading REF's section.
A document that is one empty line is replaced. [[path or title]] mentions
another document: in its own top-level folder first, then in the others (several
there: ambiguous; write [[id]]).

  quiet insert 周报 --after '#3~k7f2' --md '- [ ] 补测试'
  quiet insert 方案 --end-of 风险 --md '- 许可证需要审核，见 [[法务/清单]]'"),
    (&["replace"], "quiet replace DOC#REF [--section] --md TEXT|-

The block (--section: the heading and its section) replaced by the Markdown."),
    (&["delete"], "quiet delete DOC#REF [--section]"),
    (&["move"], "quiet move DOC#REF --after|--before|--into|--end-of REF | --start | --end [--section]

  quiet move 方案#'风险' --section --after '#2~m4qa'"),
    (&["set"], "quiet set DOC#REF [--text TEXT] [--done|--undone] [--kind text|h1…h6|bullet|ordered|todo|quote|code]

Changes one block in place: its text (inline Markdown), a todo's state, its kind.

  quiet set 发布计划#'#7~p2ka' --done"),
    (&["rename"], "quiet rename DOC TITLE

The file follows the title. Mentions in other documents keep working."),
    (&["mv"], "quiet mv DOC FOLDER

Into FOLDER, a full path within the document's own top folder (its name
alone: its top). Links to it in other documents are rewritten; moving to
another top folder is not supported yet."),
    (&["rm"], "quiet rm DOC

To the trash, where the app keeps it 30 days; `quiet untrash` brings it back."),
    (&["table"], "quiet table DOC[#REF] [--table T] [--view V] [--where 'FIELD OP VALUE']… [--any]
      [--sort FIELD[:desc]]… [--group FIELD]… [--fields A,B] [--limit N] [--offset N]
      [--stats [sum,average…]] [--as json|csv|md]
quiet table add DOC[#REF] --set Field=Value … | --rows JSON|-
quiet table update DOC[#REF] ROW --set Field=Value … | --rows JSON|-
quiet table update DOC --where COND… --set Field=Value …
quiet table delete DOC[#REF] ROW… | --where COND…
quiet table import DOC (--csv FILE|- | --rows JSON|-) [--create-fields]
quiet table field add DOC NAME --type T [--options A,B] [--formula EXPR] [--link TABLE [--two-way] [--single]]
      [--lookup LINKFIELD.FIELD [--rollup R]] [--format F] [--after FIELD]
quiet table field change DOC NAME [--name NEW] [--type T] [--options A,B] [--add-option X] [--remove-option X]
      [--rename-option OLD=NEW] [--formula EXPR] [--link TABLE] [--lookup L.F] [--format F]
quiet table field rename DOC NAME NEW  |  quiet table field delete DOC NAME…
quiet table view DOC                   the views and their settings
quiet table view add DOC NAME [--kind grid|kanban|calendar|gantt|form]
quiet table view set DOC NAME [--where COND]… [--any] [--sort F[:desc]]… [--group F]… [--hide A,B]
      [--show A,B] [--order A,B…] [--stack F] [--date F] [--end F] [--stats F=sum,…] [--clear filter|sort|group|stats|hidden]
quiet table view rename DOC NAME NEW  |  quiet table view delete DOC NAME
quiet table new DOC NAME [--fields A,B | --csv FILE|-]   another table in the file
quiet table rename DOC NAME NEW  |  quiet table remove DOC NAME [--force]

A 数据库 file, or the 数据库 block REF in a document; --table picks one of
its tables (the first by default). Rows have ids that last (r12).

Reading: every row in the table's order, or with --view a view's 筛选, 排序,
分组 and hidden fields; --where / --sort / --group add to the view (their
sorts and groups come first). OP: = != ~ (contains) !~ > >= < <= empty
notempty before after, or 等于 不等于 包含 不包含 为空 不为空 早于 晚于; only those
the field's type takes (the error lists them). Conditions hold together;
--any: any of the given ones. A date compares with a day (2026-11-26) or one
relative to today: today, yesterday, tomorrow, this_week, last_week,
next_week, this_month, last_month, next_month, past_7, next_7, past_30,
next_30 (今天, 本周, 过去 7 天内 …). Grouped output comes in sections (JSON:
groups with field, value, count, rows). --stats gives each column's 统计
(numbers sum/average/min/max, dates earliest/latest, checkboxes
checked/unchecked, the rest filled/unique, or the view's own), or the ones
named. --as csv|md prints the shown rows and fields as CSV or a Markdown
table. The output lists the file's tables and views, and each field's type,
options, formula, link or lookup.

Writing: values are text as the app shows them, written as if pasted:
numbers, dates, checkboxes (true/false), select options (new ones are
added), several joined with commas; a link (关联) takes the other table's
rows by their first field's text, or by id (r12). Computed fields
(formula, lookup, auto_number, created/modified time) cannot be written.
--rows takes [{\"Field\": value}]; for update each needs \"_id\". import
adds rows by column name: a column no field has is an error unless
--create-fields (its type guessed from the values).

Fields: types by key or the app's name (single_select or 单选, link or 关联…):
text single_select multi_select person date attachment number checkbox url
formula lookup auto_number phone email location barcode progress currency
rating link created_time modified_time. A formula is checked before it is
saved (bad_formula, with where). A type change converts the cells and says
how many could not be. --format: plain, thousands, percent, a digit
(decimals), currency=$, stars=N, prefix=X, time, a date format (dash, cn…).
The first field cannot be deleted; the last view and the last table cannot
be removed; a table other tables link to needs --force (those links turn to
text). The subcommand words come first: a document called view, new, field…
is named by its file name (view.sqlite) or path.

  quiet table 书单 --view 在读 --where '评分 >= 9' --sort 评分:desc
  quiet table 书单 --where '读完 = this_month' --stats --as md
  quiet table update 书单 --where '状态 = 想读' --set 状态=在读
  quiet table field add 书单 作家 --type link --link 作者 --two-way
  quiet table view set 书单 看板 --group 状态 --hide 备注"),
    (&["board"], "quiet board DOC[#REF] [--full]
quiet board add DOC[#REF] --elements JSON|- [--replace]
quiet board rm DOC[#REF] ID…

A 画板 file, or the board block REF in a document. `board` lists elements
(id, type, position, size, text or label, arrow ends) and their bounds;
--full adds the scene as Excalidraw JSON. `board add` takes Excalidraw
element skeletons, as convertToExcalidrawElements does: type (rectangle,
ellipse, diamond, text, arrow, line, frame), x, y, optional width, height,
label: {text}, text (for text), strokeColor, backgroundColor, id (to point
arrows at), start/end: {id} for arrows. New elements go on top; --replace
clears the board first.

  quiet board add 架构图 --elements '[{\"type\":\"rectangle\",\"id\":\"a\",\"x\":0,\"y\":0,\"label\":{\"text\":\"App\"}},{\"type\":\"rectangle\",\"id\":\"b\",\"x\":300,\"y\":0,\"label\":{\"text\":\"CLI\"}},{\"type\":\"arrow\",\"x\":100,\"y\":35,\"start\":{\"id\":\"a\"},\"end\":{\"id\":\"b\"}}]'"),
    (&["history"], "quiet history DOC [VERSION]

The versions kept of DOC, newest first (each save keeps the text it
replaced). With VERSION (its name, or 1 for the newest, 2 …): what changed
from it to now."),
    (&["restore"], "quiet restore DOC VERSION

DOC put back as it was at VERSION. What it has now is kept as a version
first, so a restore can itself be undone."),
    (&["trash"], "quiet trash

What is in the trash (kept 30 days), with ids for `quiet untrash`. Each top-level
folder has its own; untrash puts a document back where it was."),
    (&["untrash"], "quiet untrash NAME

Brings the document called NAME (or with that id) back from the trash, to where it was."),
    (&["import"], "quiet import FILE… [--in FOLDER]

Copies files into the library: Markdown (.md, .txt; pictures beside it are
copied in), DocxXML (.xml), CSV/TSV (as a 数据库), .excalidraw (as a 画板).
The originals are left alone."),
    (&["run"], "quiet run [FILE|-] [-e CODE] [--timeout SECS] [--dry-run]

A JavaScript script against `lib` (see `quiet guide`, \"Script API\"). The
whole script is one transaction: if it throws or times out (60 s), nothing
is written. Top-level await and return work; a returned value is printed.

  quiet run -e 'return lib.find(\"预算\").results.map(r => r.path)'"),
    (&["call"], "quiet call OP [JSON|-]

Any operation by its name with JSON arguments, as `lib.call` in a script.
Operations: ls find load outline read blocks links check history trash_list
(reading); insert replace remove move set create mkdir rename mv trash
restore untrash import table table_add table_update table_delete
table_import table_field_add table_field_change table_field_delete
table_views table_view_add table_view_set table_view_rename
table_view_delete table_new table_rename table_remove board board_add
board_remove. Arguments are the commands' options by name, the
document as \"doc\" and a block as \"at\". `load` is what lib.doc does:
a document's path, title and rev loaded into the script (not shown in the
app; that is `quiet open`).

  quiet call blocks '{\"doc\": \"周报\", \"kind\": \"todo\", \"done\": false}'"),
    (&["mcp"], "quiet mcp [--library DIR]

An MCP server on stdin/stdout with two tools: `quiet` (a command line) and
`quiet_run` (a script). For agents that speak MCP but have no shell. E.g. in
an MCP client's settings: {\"command\": \"quiet\", \"args\": [\"mcp\"]}"),
    (&["folders"], "quiet folders [ls] | quiet folders add PATH [--force] | quiet folders remove NAME|PATH|KEY

The top-level folders Quiet.md's sidebar lists on this computer (主页), as
添加文件夹 / 从侧栏移除 in the app keep them: each line is name, path on disk,
key, and `default` for the one new things go to. `add` opens a folder where it
is (nothing is copied; a hidden .quiet folder is made in it); it must exist,
not be open already, not be inside one that is open, not hold one, and not be
a disk's top, the home folder or a system folder. A code project or a folder
with a great many files asks first, as the app does: --force adds it anyway.
`remove` takes it off the sidebar; its files stay as they are. A running
Quiet.md shows the change within a few seconds. --dry-run checks, writes nothing.

  quiet folders add ~/Documents/Notes
  quiet folders remove Notes"),
    (&["open"], "quiet open DOC[#REF] [--dry-run] | quiet open --folder FOLDER | quiet open --trash

Shows the document in Quiet.md, scrolled to the block REF names and with that
block selected (REF as `quiet read` takes it: #3~k7f2, #2.1 or a heading's
text), a folder's page (a top-level folder's name opens its own), or the
trash (after `quiet rm`, say). `quiet new … --open` opens what it made. Quiet.md
is launched if it isn't running. It goes through a quietmd:// link, which only
navigates: it prints the link; --dry-run prints it without opening it. The
link goes to the installed Quiet.md (a development preview does not take them). With
--library DIR, that folder must be open in Quiet.md (`quiet folders add`).

  quiet open 工作/周报#风险
  quiet open --folder 工作/项目"),
    (&["duplicate", "cp"], "quiet duplicate DOC [TITLE]

创建副本: a copy of the document (a 画板, a 数据库, a folder's 正文 too) beside
it, titled TITLE or \"<title> 副本\" as the app names it. Links inside the
copy still point where they pointed.

  quiet duplicate 工作/周报 \"周报 模板\""),
    (&["pin", "unpin"], "quiet pin DOC | quiet unpin DOC

添加到「置顶」/ 从「置顶」移除: the document shows in the sidebar's 置顶
list (`ls --json` says `pinned`). The file itself is not touched.

  quiet pin 工作/周报"),
    (&["telemetry"], "quiet telemetry [on|off|status]

quiet counts how often its commands run: per day, how many ran and failed and
which commands (by name), under a random id for this computer. Never the
library, documents, titles or arguments. Finished days are sent once a day to
telemetry.quiet.md, with quiet's version and the system (macos/windows, arm or
x86). `status` shows what is kept and waiting; `off` stops it and forgets the
id. Nothing is counted while DO_NOT_TRACK or QUIET_NO_TELEMETRY is set, in CI,
or while Settings › General › Anonymous usage statistics (设置 › 通用 › 匿名使用统计)
is off in Quiet.md (untouched, it follows Check for updates automatically)."),
    (&["guide", "skill"], "quiet guide

The full guide for agents: addressing, every command, the script API,
examples. It matches this quiet; the agent skill (npx skills add
s1dashu/quiet-md) carries the same guide as its reference."),
];

pub fn of(cmd: Option<&str>) -> R<String> {
    let Some(c) = cmd else { return Ok(format!("{HELP}\n")) };
    COMMANDS.iter().find(|(names, _)| names.contains(&c)).map(|(_, t)| format!("{t}\n"))
        .ok_or_else(|| fail("usage", format!("no command {c:?}")).hint("`quiet help` lists them"))
}
