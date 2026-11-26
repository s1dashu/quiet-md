---
name: quiet-md
description: Read, search, edit and organize the user's Quiet.md notes library (documents, Markdown notes, boards, 数据表 data tables) with the `quiet` command line tool. Use it whenever a task touches the user's notes or knowledge base — "my notes", "my library", "Quiet.md", 笔记, 笔记库, 文档库, 知识库, 周报, 会议纪要, 待办, 数据表 — to find or summarize something in them, write a new note, update or reorganize existing ones, collect todos, query or fill a data table, or draw a diagram on a board. Edit the library through `quiet`, not by writing its files directly.
---

# Quiet.md: the user's notes library

Quiet.md keeps notes as plain files in the folders the user opened in it:
`.xml` documents, `.md` notes, `.excalidraw` boards and `.sqlite` 数据表. The
user edits them in the app; you work on the same files with `quiet`, which
keeps them valid, rewrites links when a document moves, keeps every replaced
text as a version, and refuses to overwrite a change made since you read.

## First: is `quiet` there?

Run `quiet version`. If it is not found, it ships inside the Quiet.md app:

- **macOS**: ask the user to click Settings › Agent Connector › Install Command (设置 › 连接 Agent › 安装命令行工具) in
  Quiet.md (it links `quiet` into `~/.local/bin`), or do it yourself:
  `mkdir -p ~/.local/bin && ln -sf /Applications/quiet.md.app/Contents/MacOS/quiet ~/.local/bin/quiet`
  (the app may be in `~/Applications` instead). If `~/.local/bin` is not on
  the PATH, add `export PATH="$HOME/.local/bin:$PATH"` to `~/.zshrc` (or the
  user's shell profile) and call `~/.local/bin/quiet` until a new shell.
- **Windows**: the installer puts `quiet.exe` on the user's PATH (open a new
  terminal); otherwise it is in the install folder beside `quiet_md.exe`,
  `%LOCALAPPDATA%\Programs\quiet.md\` by default.
- **Linux**: `bin/quiet` in the `.tar.gz` package; the AppImage runs it when
  called through a link named `quiet` (`ln -sf /path/to/quiet-md-*.AppImage ~/.local/bin/quiet`).
- No Quiet.md, or an app without `quiet` in it: ask the user to download
  the latest Quiet.md from https://quiet.md and open it once.

Then `quiet ls` lists the folders the app has open: you are set.

## Commands

```
quiet ls [FOLDER] [-r] [--kind doc|md|board|table]   what is there (no folder: the top folders)
quiet find WORDS… [--in FOLDER]        documents holding every word, with matching blocks and refs
quiet outline DOC                      headings with refs and sizes: read this before a long document
quiet read DOC#REF --section           one section (refs look like #3~k7f2); the whole: quiet read DOC
quiet blocks DOC --kind todo --undone  blocks by kind, text, state
quiet new TITLE [--in FOLDER] --md '…' a new document from Markdown (--kind md|board|table)
quiet insert DOC --end-of HEADING --md '…'   add to a section (or --after REF, --start, --end)
quiet replace DOC#REF --section --md '…'     rewrite a block or a heading's section
quiet set DOC#REF --text '…' | --done  change one block
quiet run script.js                    a JavaScript script against `lib`: many steps, one transaction
quiet folders add PATH                 put a folder in the app's sidebar (where it is); folders remove NAME
quiet open DOC#REF                     show the user a document in the app, at that block
```

Paths start at a top folder's name (`Quiet.md/工作/周报`, extension optional);
a title only one document has works too. New content is Markdown;
`[[path or title]]` links another document. Reads print the document's
**rev**: pass it back with `--expect-rev REV` when you write. Every write
takes `--dry-run`. Use `quiet run` instead of many commands when the steps
do not need your judgement in between.

**数据表.** Rows have ids (`r12`); values are text as the app shows them and
are written as if pasted.

```
quiet table 书单                                      tables, views, fields and rows
quiet table 书单 --view 在读 --where '评分 >= 8' --sort 评分:desc --fields 书名,评分
quiet table 书单 --group 状态 --stats --as md         grouped, with each column's 统计
quiet table add 书单 --set 书名=三体 --set 状态=在读
quiet table update 书单 --where '状态 = 想读' --set 状态=在读 --dry-run
quiet table delete 书单 r12
quiet table field add 书单 评分 --type rating          fields, views, tables: quiet help table
```

For everything else (the script API, conditions and date ranges, boards,
history, imports) read `references/guide.md` next to this file, or run
`quiet guide`; `quiet help COMMAND` shows one command.

## Working in someone's library

These are the user's own notes; they may have the app open while you work.

- Read before you write: `outline`, then only the sections you need.
- Prefer adding over rewriting. Do not delete, move, rename, restore or
  rewrite whole sections of existing documents unless the user asked for it.
- For a change across several documents, or a large one, run it with
  `--dry-run` first and show the user the diff when it is not clearly what they asked.
- Put new documents where the user said; if they did not say, where related
  documents are, and tell them where.
- After writing, tell the user what changed: the paths, and what was added or changed.
- If you got it wrong, undo it: `quiet history DOC`, then `quiet restore DOC 1`.
- On `stale_ref`, `stale_rev` or `conflict` the document changed since you read
  it (the user may be typing in it): read it again and redo the edit; never force it.
- Never write the library's files directly, and leave its `.quiet/` folder alone.
