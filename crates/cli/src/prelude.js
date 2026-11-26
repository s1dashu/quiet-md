// The script API of `quiet run`, over one native call: __q(op, argsJson)
// runs a Session operation and answers {ok} or {error}. Kept thin on
// purpose: every method here is a command of the CLI under another name.
"use strict";
const __call = (op, args) => {
  const r = JSON.parse(__q(op, JSON.stringify(args ?? {})));
  if (r.error) { const e = new Error(r.error.message); e.code = r.error.code; if (r.error.hint) e.hint = r.error.hint; throw e; }
  return r.ok;
};
const __anchor = (x) => x == null ? undefined : typeof x === "object" ? (x.id ?? x.ref) : String(x);
const __where = (w) => {
  if (w == null || w === "end") return { to: "end" };
  if (w === "start") return { to: "start" };
  if (typeof w === "object" && !w.id && !w.ref) {
    const o = {};
    for (const k of ["after", "before", "into"]) if (w[k] != null) o[k] = __anchor(w[k]);
    if (w.endOf != null) o.end_of = __anchor(w.endOf);
    if (w.to) o.to = w.to;
    if (Object.keys(o).length) return o;
  }
  throw Object.assign(new TypeError(`where: use "start", "end", {after: ref}, {before: ref}, {into: ref} or {endOf: heading}`), { code: "bad_args" });
};
// which 数据表: a datatable block (`at`) and a table of its file (`table`)
const __t = (o) => ({ at: __anchor(o.at), table: o.table });
const __fmt = (v) => typeof v === "string" ? v : v === undefined ? "undefined" : JSON.stringify(v);
globalThis.console = { log: (...a) => __print(a.map(__fmt).join(" ")), info: (...a) => __print(a.map(__fmt).join(" ")), warn: (...a) => __print(a.map(__fmt).join(" ")), error: (...a) => __print(a.map(__fmt).join(" ")) };
globalThis.print = console.log;

class Doc {
  constructor(info) { Object.assign(this, info); }
  _call(op, a = {}) { return __call(op, { ...a, doc: this.key }); }
  outline() { return this._call("outline"); }
  markdown(at, o = {}) { return this._call("read", { as: "md", at: __anchor(at), section: o.section, heading: o.heading }).text; }
  text(at, o = {}) { return this._call("read", { as: "text", at: __anchor(at), section: o.section, heading: o.heading }).text; }
  blocks(f = {}) { return this._call("blocks", { kind: f.kind, grep: f.grep, done: f.done, at: __anchor(f.within) }).blocks; }
  block(at) { return this._call("read", { as: "json", at: __anchor(at) }).blocks[0]; }
  section(at) { return this._call("read", { as: "json", at: __anchor(at), section: true }).blocks; }
  insert(md, where) { return this._call("insert", { md, ...__where(where) }).inserted; }
  append(md) { return this.insert(md, "end"); }
  prepend(md) { return this.insert(md, "start"); }
  replace(at, md, o = {}) { return this._call("replace", { at: __anchor(at), md, section: o.section }).inserted; }
  remove(at, o = {}) { return this._call("remove", { at: __anchor(at), section: o.section }).removed; }
  move(at, where, o = {}) { return this._call("move", { at: __anchor(at), ...__where(where), section: o.section }).moved; }
  setText(at, text) { this._call("set", { at: __anchor(at), text }); }
  setDone(at, done = true) { this._call("set", { at: __anchor(at), done }); }
  setKind(at, kind) { this._call("set", { at: __anchor(at), kind }); }
  rename(title) { this._call("rename", { title }); }
  moveTo(folder) { this._call("mv", { folder: folder ?? "/" }); }
  trash() { this._call("trash"); }
  pin(on = true) { this._call(on ? "pin" : "unpin"); }
  duplicate(title) { this._call("duplicate", { title }); }
  links() { return this._call("links"); }
  table(o = {}) { return this._call("table", { ...__t(o), view: o.view, where: o.where, any: o.any, sort: o.sort, group: o.group, fields: o.fields, stats: o.stats, as: o.as, limit: o.limit, offset: o.offset }); }
  addRows(rows, o = {}) { return this._call("table_add", { rows, at: __anchor(o.at), table: o.table }).added; }
  updateRows(rows, o = {}) { return this._call("table_update", { rows, at: __anchor(o.at), table: o.table }).updated; }
  updateRow(row, set, o = {}) { return this.updateRows([{ ...set, _id: typeof row === "object" ? row._id : row }], o); }
  deleteRows(rows, o = {}) { return this._call("table_delete", { rows: [].concat(rows).map((r) => typeof r === "object" ? r._id : r), at: __anchor(o.at), table: o.table }).deleted; }
  updateWhere(where, set, o = {}) { return this._call("table_update", { ...__t(o), where, set, any: o.any, view: o.view }).updated; }
  deleteWhere(where, o = {}) { return this._call("table_delete", { ...__t(o), where, any: o.any, view: o.view }).rows; }
  importRows(rows, o = {}) { return this._call("table_import", { ...__t(o), ...(typeof rows === "string" ? { csv: rows } : { rows }), create_fields: o.createFields }); }
  addField(name, type, o = {}) { return this._call("table_field_add", { ...__t(o), name, type, options: o.options, formula: o.formula, link: o.link, lookup: o.lookup, rollup: o.rollup, format: o.format, after: o.after, description: o.description, two_way: o.twoWay, single: o.single }); }
  changeField(name, c = {}) { return this._call("table_field_change", { ...c, ...__t(c), field: name, add_options: c.addOptions, remove_options: c.removeOptions, rename_options: c.renameOptions }); }
  deleteFields(names, o = {}) { return this._call("table_field_delete", { ...__t(o), fields: [].concat(names) }); }
  views(o = {}) { return this._call("table_views", __t(o)).views; }
  addView(name, o = {}) { return this._call("table_view_add", { ...__t(o), name, kind: o.kind }).added; }
  setView(name, s = {}) { return this._call("table_view_set", { ...s, ...__t(s), view: name }).view; }
  renameView(name, to, o = {}) { this._call("table_view_rename", { ...__t(o), view: name, name: to }); }
  deleteView(name, o = {}) { this._call("table_view_delete", { ...__t(o), view: name }); }
  addTable(name, o = {}) { return this._call("table_new", { at: __anchor(o.at), name, fields: o.fields, csv: o.csv }); }
  renameTable(name, to, o = {}) { this._call("table_rename", { at: __anchor(o.at), table: name, name: to }); }
  removeTable(name, o = {}) { return this._call("table_remove", { at: __anchor(o.at), table: name, force: o.force }); }
  board(o = {}) { return this._call("board", { at: __anchor(o.at), full: o.full }); }
  addToBoard(elements, o = {}) { return this._call("board_add", { elements, at: __anchor(o.at), replace: o.replace }).added; }
  removeFromBoard(ids, o = {}) { return this._call("board_remove", { ids: [].concat(ids), at: __anchor(o.at) }).removed; }
  history(version) { return this._call("history", { version: version == null ? undefined : String(version) }); }
  restore(version) { this._call("restore", { version: String(version) }); }
}

globalThis.lib = {
  ls(folder, o = {}) { if (typeof folder === "object" && folder) { o = folder; folder = o.folder; } return __call("ls", { folder, recursive: o.recursive, kind: o.kind }); },
  find(query, o = {}) { return __call("find", { query, folder: o.folder ?? o.in, limit: o.limit }); },
  doc(path, o = {}) { return new Doc(__call("open", { doc: path, rev: o.rev })); },
  create(title, o = {}) { const r = __call("create", { title, folder: o.folder ?? o.in, md: o.md, markdown: o.markdown, kind: o.kind, csv: o.csv, fields: o.fields }); return r.key ? new Doc(r) : r; },
  mkdir(name, o = {}) { return __call("mkdir", { name, folder: o.folder ?? o.in }); },
  check() { return __call("check"); },
  trash() { return __call("trash_list").items; },
  untrash(name) { return __call("untrash", { name }); },
  call: __call,
};

globalThis.help = (name) => {
  const g = __guide();
  const api = g.slice(g.indexOf("## Script API"), g.indexOf("## ", g.indexOf("## Script API") + 3));
  if (!name) return __print(api.trim());
  const lines = api.split("\n").filter((l) => l.includes(name));
  __print(lines.length ? lines.join("\n") : `no API entry mentions ${name}; help() lists them all`);
};
