//! `quiet table …`: 数据表 read through views and conditions, and their
//! fields, views and tables changed, against libraries in temporary folders.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use fdl_core::store::Store;
use fdl_core::table::{field, sqlite};
use serde_json::Value;

static N: AtomicU32 = AtomicU32::new(0);

fn library() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("quiet-tables-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    Store::open_at(dir.clone()).unwrap();
    dir
}

fn quiet(lib: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quiet")).env("QUIET_NO_TELEMETRY", "1").arg("--library").arg(lib).args(args).output().unwrap()
}

fn ok(lib: &Path, args: &[&str]) -> String {
    let o = quiet(lib, args);
    assert!(o.status.success(), "quiet {args:?} failed: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

fn json(lib: &Path, args: &[&str]) -> Value {
    let mut a = vec!["--json"];
    a.extend_from_slice(args);
    serde_json::from_str(&ok(lib, &a)).unwrap()
}

/// The command fails with `code` and exit status `exit`; its message.
fn fails(lib: &Path, args: &[&str], code: &str, exit: i32) -> String {
    let o = quiet(lib, args);
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    assert_eq!(o.status.code(), Some(exit), "quiet {args:?}: {err}");
    assert!(err.starts_with(&format!("error[{code}]")), "quiet {args:?}: {err}");
    err
}

fn col(t: &Value, name: &str) -> Vec<String> { t["rows"].as_array().unwrap().iter().map(|r| r[name].as_str().unwrap().to_string()).collect() }

const BOOKS: &str = "书名,作者,状态,页数,评分,读完\n三体,刘慈欣,在读,302,9.5,2026-11-20\n活着,余华,读完,191,9.1,2026-10-01\n百年孤独,马尔克斯,想读,360,9.3,\n围城,钱锺书,读完,359,8.9,2026-11-25\n红楼梦,曹雪芹,想读,1600,9.6,\n平凡的世界,路遥,在读,1200,9.0,\n";

/// 书单: six books, 状态 a 单选.
fn books() -> PathBuf {
    let lib = library();
    ok(&lib, &["new", "书单", "--kind", "table", "--csv", BOOKS]);
    ok(&lib, &["table", "field", "change", "书单", "状态", "--type", "单选"]);
    lib
}

#[test]
fn reads_a_view_filtered_sorted_and_grouped() {
    let lib = books();
    ok(&lib, &["table", "view", "add", "书单", "书架"]);
    let set = json(&lib, &["table", "view", "set", "书单", "书架", "--where", "页数 < 1000", "--sort", "评分:desc", "--group", "状态", "--hide", "作者"]);
    assert_eq!(set["view"]["where"], "页数 < 1000");
    let t = json(&lib, &["table", "书单", "--view", "书架"]);
    assert_eq!((t["view"].as_str(), t["total"].as_u64(), t["records"].as_u64()), (Some("书架"), Some(4), Some(6)));
    assert!(!t["fields"].as_array().unwrap().contains(&Value::from("作者")), "hidden in the view");
    // groups in the options' order, each sorted by 评分 descending
    let groups: Vec<(String, Vec<String>)> = t["groups"].as_array().unwrap().iter().map(|g| (g["value"].as_str().unwrap().into(), g["rows"].as_array().unwrap().iter().map(|r| r["书名"].as_str().unwrap().into()).collect())).collect();
    assert_eq!(groups, [("在读".into(), vec!["三体".to_string()]), ("读完".into(), vec!["活着".into(), "围城".into()]), ("想读".into(), vec!["百年孤独".into()])]);
    assert_eq!(t["views"][1], serde_json::json!({ "name": "书架", "kind": "grid" }));
    let text = ok(&lib, &["table", "书单", "--view", "书架"]);
    assert!(text.contains("## 状态 = 读完  (2 rows)") && text.contains("where: 页数 < 1000"), "{text}");
    // without a view: every row, in the table's order
    assert_eq!(col(&json(&lib, &["table", "书单"]), "书名"), ["三体", "活着", "百年孤独", "围城", "红楼梦", "平凡的世界"]);
    // paging a grouped read keeps the group counts
    let page = json(&lib, &["table", "书单", "--view", "书架", "--limit", "2"]);
    assert_eq!((page["groups"].as_array().unwrap().len(), page["groups"][1]["count"].as_u64(), page["more"].as_u64()), (2, Some(2), Some(2)));
}

#[test]
fn conditions_lay_over_a_view() {
    let lib = books();
    ok(&lib, &["table", "view", "add", "书单", "好书"]);
    ok(&lib, &["table", "view", "set", "书单", "好书", "--where", "评分 >= 9.5", "--any", "--where", "状态 = 读完"]);
    assert_eq!(col(&json(&lib, &["table", "书单", "--view", "好书"]), "书名"), ["三体", "活着", "围城", "红楼梦"]);
    // the view's 或 stays whole, the new condition must hold as well
    let t = json(&lib, &["table", "书单", "--view", "好书", "--where", "页数 < 1000", "--sort", "页数"]);
    assert_eq!(col(&t, "书名"), ["活着", "三体", "围城"]);
    assert_eq!(t["where"], "(评分 >= 9.5 or 状态 = 读完) and 页数 < 1000");
    // --any joins the given ones with or; Chinese operators read too
    assert_eq!(col(&json(&lib, &["table", "书单", "--where", "作者 包含 余", "--any", "--where", "书名 等于 围城"]), "书名"), ["活着", "围城"]);
    assert_eq!(col(&json(&lib, &["table", "书单", "--where", "读完 为空", "--where", "状态 != 在读"]), "书名"), ["百年孤独", "红楼梦"]);
    // and in a script, as objects
    let o = quiet(&lib, &["run", "-e", "return lib.doc('书单').table({ view: '好书', where: { field: '页数', op: '>', value: 1000 } }).rows.map(r => r.书名)"]);
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "[\"红楼梦\"]", "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn dates_filter_by_days_relative_to_today() {
    let lib = library();
    let day = |n: i64| field::iso(field::today() + n * 86_400_000, false);
    let csv = format!("事,日期\n今天的,{}\n昨天的,{}\n上月的,{}\n没日期的,\n", day(0), day(-1), day(-40));
    ok(&lib, &["new", "日程", "--kind", "table", "--csv", &csv]);
    let rows = |w: &str| col(&json(&lib, &["table", "日程", "--where", w]), "事");
    assert_eq!(rows("日期 = today"), ["今天的"]);
    assert_eq!(rows("日期 = 昨天"), ["昨天的"]);
    assert_eq!(rows("日期 = past_7"), ["今天的", "昨天的"]);
    assert_eq!(rows("日期 = 过去 30 天内"), ["今天的", "昨天的"]);
    assert_eq!(rows("日期 before today"), ["昨天的", "上月的"]);
    assert_eq!(rows("日期 < yesterday"), ["上月的"]);
    assert_eq!(rows(&format!("日期 = {}", day(-1))), ["昨天的"]);
    assert_eq!(rows("日期 empty"), ["没日期的"]);
    assert_eq!(json(&lib, &["table", "日程", "--where", "日期 = this_week"])["where"], "日期 = this_week");
    let e = fails(&lib, &["table", "日程", "--where", "日期 = someday"], "bad_args", 2);
    assert!(e.contains("not a date") && e.contains("past_7"), "{e}");
}

#[test]
fn an_operator_a_field_does_not_take_is_refused() {
    let lib = books();
    let e = fails(&lib, &["table", "书单", "--where", "页数 ~ 3"], "bad_args", 2);
    assert!(e.contains("`~` does not apply to 页数 (number); it takes: = != > >= < <= empty notempty"), "{e}");
    let e = fails(&lib, &["table", "书单", "--where", "读完 > 3 days"], "bad_args", 2);
    assert!(e.contains("not a date"), "{e}");
    let e = fails(&lib, &["table", "书单", "--where", "状态 = 弃读"], "bad_args", 2);
    assert!(e.contains("no option \"弃读\"") && e.contains("options: 在读, 读完, 想读"), "{e}");
    fails(&lib, &["table", "书单", "--where", "评分 > high"], "bad_args", 2);
    fails(&lib, &["table", "书单", "--where", "页数 >"], "bad_args", 2);
    fails(&lib, &["table", "书单", "--where", "just words"], "bad_args", 2);
    let e = fails(&lib, &["table", "书单", "--where", "出版社 = x"], "not_found", 3);
    assert!(e.contains("fields: 书名, 作者, 状态, 页数, 评分, 读完"), "{e}");
    fails(&lib, &["table", "书单", "--view", "没有"], "not_found", 3);
    fails(&lib, &["table", "书单", "--sort", "没有:desc"], "not_found", 3);
}

#[test]
fn stats_by_type_or_as_asked() {
    let lib = books();
    let st = |t: &Value, f: &str| t["stats"].as_array().unwrap().iter().find(|s| s["field"] == f).cloned().unwrap_or(Value::Null);
    let t = json(&lib, &["table", "书单", "--stats", "--where", "状态 = 读完"]);
    assert_eq!(st(&t, "页数"), serde_json::json!({ "field": "页数", "sum": "550", "average": "275", "min": "191", "max": "359" }));
    assert_eq!(st(&t, "读完"), serde_json::json!({ "field": "读完", "earliest": "2026/10/01", "latest": "2026/11/25" }));
    assert_eq!(st(&t, "状态"), serde_json::json!({ "field": "状态", "filled": "2", "unique": "1" }));
    let t = json(&lib, &["table", "书单", "--stats", "sum,max"]);
    assert_eq!(st(&t, "页数"), serde_json::json!({ "field": "页数", "sum": "4012", "max": "1600" }));
    assert_eq!(st(&t, "书名"), Value::Null, "a text field has no sum");
    // a view's own 统计
    ok(&lib, &["table", "view", "set", "书单", "表格视图", "--stats", "页数=average"]);
    assert_eq!(st(&json(&lib, &["table", "书单", "--view", "表格视图", "--stats"]), "页数"), serde_json::json!({ "field": "页数", "average": "668.67" }));
    let text = ok(&lib, &["table", "书单", "--stats", "sum", "--limit", "0"]);
    assert!(text.contains("stats:\n  页数: sum 4012\n"), "{text}");
    fails(&lib, &["table", "书单", "--stats", "median"], "bad_args", 2);
    fails(&lib, &["table", "view", "set", "书单", "表格视图", "--stats", "书名=sum"], "bad_args", 2);
}

#[test]
fn exports_csv_and_markdown_as_the_view_shows() {
    let lib = books();
    let csv = ok(&lib, &["table", "书单", "--where", "状态 = 读完", "--fields", "书名,评分", "--as", "csv"]);
    assert_eq!(csv, "书名,评分\n活着,9.1\n围城,8.9\n");
    let md = ok(&lib, &["table", "书单", "--sort", "页数:desc", "--fields", "书名", "--limit", "2", "--as", "md"]);
    assert_eq!(md, "| 书名 |\n| --- |\n| 红楼梦 |\n| 平凡的世界 |\n");
    assert_eq!(json(&lib, &["table", "书单", "--as", "csv", "--fields", "书名", "--limit", "1"])["text"], "书名\n三体\n");
    let with = ok(&lib, &["table", "书单", "--fields", "书名,页数", "--as", "md", "--stats", "sum"]);
    assert!(with.ends_with("| 平凡的世界 | 1200 |\nstats:\n  页数: sum 4012\n"), "{with}");
}

#[test]
fn values_read_as_the_app_shows_them() {
    let lib = books();
    ok(&lib, &["table", "field", "add", "书单", "价格", "--type", "currency", "--format", "$,1"]);
    ok(&lib, &["table", "field", "add", "书单", "进度", "--type", "进度"]);
    ok(&lib, &["table", "field", "change", "书单", "读完", "--format", "cn"]);
    ok(&lib, &["table", "update", "书单", "--where", "书名 = 三体", "--set", "价格=1234.5", "--set", "进度=40"]);
    let t = json(&lib, &["table", "书单"]);
    // what the app's grid shows: the engine's own display of each cell
    let base = fdl_core::table::Doc::new(sqlite::load(&lib.join("书单.sqlite")).unwrap()).base;
    let tb = &base.tables[0];
    for (r, row) in tb.records.iter().zip(t["rows"].as_array().unwrap()) {
        assert_eq!(row["_id"], format!("r{}", r.id));
        for f in &tb.fields { assert_eq!(row[&f.name], field::display_in(&base, f, r.get(f.id)), "{}", f.name); }
    }
    assert_eq!((t["rows"][0]["价格"].as_str(), t["rows"][0]["进度"].as_str(), t["rows"][0]["读完"].as_str()), (Some("$1,234.5"), Some("40%"), Some("2026年11月20日")));
    let price = t["schema"].as_array().unwrap().iter().find(|f| f["name"] == "价格").unwrap();
    assert_eq!(price["format"], serde_json::json!({ "currency": "$", "precision": 1 }));
}

#[test]
fn fields_of_every_kind_are_added() {
    let lib = books();
    ok(&lib, &["table", "new", "书单", "作者", "--fields", "姓名,国家"]);
    ok(&lib, &["table", "import", "书单", "--table", "作者", "--rows", r#"[{"姓名":"刘慈欣","国家":"中国"},{"姓名":"余华","国家":"中国"},{"姓名":"马尔克斯","国家":"哥伦比亚"}]"#]);
    let a = json(&lib, &["table", "field", "add", "书单", "标签", "--type", "multi_select", "--options", "科幻,经典", "--after", "书名"]);
    assert_eq!(a["added"], "标签");
    ok(&lib, &["table", "field", "add", "书单", "每百页", "--type", "公式", "--formula", "ROUND({页数} / 100, 1)", "--format", "1"]);
    let l = json(&lib, &["table", "field", "add", "书单", "作家", "--type", "link", "--link", "作者", "--two-way"]);
    assert_eq!(l["mirror"], "作者.书单");
    ok(&lib, &["table", "field", "add", "书单", "国家", "--type", "lookup", "--lookup", "作家.国家"]);
    ok(&lib, &["table", "update", "书单", "--where", "书名 = 百年孤独", "--set", "作家=马尔克斯, 余华", "--set", "标签=经典"]);
    let first = json(&lib, &["table", "书单", "--table", "作者", "--where", "姓名 = 刘慈欣"])["rows"][0]["_id"].as_str().unwrap().to_string();
    ok(&lib, &["table", "update", "书单", "--where", "书名 = 三体", "--set", &format!("作家={first}"), "--set", "标签=科幻,新标签"]);
    let t = json(&lib, &["table", "书单", "--where", "作家 notempty", "--fields", "书名,标签,作家,国家,每百页"]);
    assert_eq!(col(&t, "作家"), ["刘慈欣", "马尔克斯, 余华"]);
    assert_eq!(col(&t, "国家"), ["中国", "哥伦比亚, 中国"]);
    assert_eq!(col(&t, "标签"), ["科幻, 新标签", "经典"]);
    assert_eq!(col(&t, "每百页"), ["3.0", "3.6"]);
    let schema: Vec<Value> = json(&lib, &["table", "书单"])["schema"].as_array().unwrap().clone();
    let f = |n: &str| schema.iter().find(|f| f["name"] == n).unwrap().clone();
    assert_eq!(schema[1]["name"], "标签", "--after 书名");
    assert_eq!(f("标签")["options"], serde_json::json!(["科幻", "经典", "新标签"]));
    assert_eq!((f("每百页")["formula"].as_str(), f("每百页")["computed"].as_bool()), (Some("ROUND({页数} / 100, 1)"), Some(true)));
    assert_eq!((f("作家")["link"].as_str(), f("作家")["mirror"].as_str()), (Some("作者"), Some("书单")));
    assert_eq!((f("国家")["lookup"].as_str(), f("国家")["rollup"].as_str()), (Some("作家.国家"), Some("values")));
    // the mirror holds the books back
    let back = json(&lib, &["table", "书单", "--table", "作者", "--where", "姓名 = 余华"]);
    assert_eq!(back["rows"][0]["书单"], "百年孤独");
    // a 关联 value no row has, or two rows have, is refused
    let e = fails(&lib, &["table", "update", "书单", "--where", "书名 = 活着", "--set", "作家=莫言"], "not_found", 3);
    assert!(e.contains("no row \"莫言\" in 作者"), "{e}");
    ok(&lib, &["table", "add", "书单", "--table", "作者", "--set", "姓名=余华"]);
    fails(&lib, &["table", "update", "书单", "--where", "书名 = 活着", "--set", "作家=余华"], "ambiguous", 3);
    // what cannot be added
    fails(&lib, &["table", "field", "add", "书单", "评分", "--type", "number"], "exists", 1);
    fails(&lib, &["table", "field", "add", "书单", "x", "--type", "lookup", "--lookup", "书名.国家"], "bad_args", 2);
    fails(&lib, &["table", "field", "add", "书单", "x", "--type", "formula"], "bad_args", 2);
    fails(&lib, &["table", "field", "add", "书单", "x", "--type", "colour"], "bad_args", 2);
    fails(&lib, &["table", "field", "add", "书单", "x", "--type", "number", "--options", "a"], "bad_args", 2);
    fails(&lib, &["table", "update", "书单", "--where", "书名 = 活着", "--set", "国家=中国"], "read_only", 1);
}

#[test]
fn a_formula_that_does_not_parse_says_where() {
    let lib = books();
    let e = fails(&lib, &["table", "field", "add", "书单", "密度", "--type", "formula", "--formula", "{评分} / {页码}"], "bad_formula", 2);
    assert!(e.contains("页码") && e.contains("at 8–11 (\"{页码}\")") && e.contains("fields: 书名"), "{e}");
    let e = fails(&lib, &["table", "field", "add", "书单", "密度", "--type", "formula", "--formula", "ROUND({评分}, "], "bad_formula", 2);
    assert!(e.contains(" at "), "{e}");
    assert_eq!(json(&lib, &["table", "书单"])["schema"].as_array().unwrap().len(), 6, "nothing was added");
}

#[test]
fn a_type_change_converts_and_reports_what_it_lost() {
    let lib = books();
    ok(&lib, &["table", "add", "书单", "--set", "书名=无名", "--set", "页数=很多"]);
    let c = json(&lib, &["table", "field", "change", "书单", "作者", "--type", "number"]);
    assert_eq!((c["converted"].as_u64(), c["lost"].as_u64()), (Some(0), Some(6)));
    assert_eq!(c["lost_rows"][0]["was"], "刘慈欣");
    let c = json(&lib, &["table", "field", "change", "书单", "页数", "--type", "text"]);
    assert_eq!((c["converted"].as_u64(), c["lost"].as_u64()), (Some(6), Some(0)));
    let c = json(&lib, &["table", "field", "change", "书单", "页数", "--type", "number", "--name", "页"]);
    assert_eq!((c["field"].as_str(), c["converted"].as_u64(), c["lost"].as_u64()), (Some("页"), Some(6), Some(0)));
    // the primary field stays something a title can be, and stays
    let e = fails(&lib, &["table", "field", "change", "书单", "书名", "--type", "checkbox"], "bad_args", 2);
    assert!(e.contains("primary field"), "{e}");
    let e = fails(&lib, &["table", "field", "delete", "书单", "书名"], "bad_args", 2);
    assert!(e.contains("书名 is the primary field"), "{e}");
    fails(&lib, &["table", "field", "rename", "书单", "页", "评分"], "exists", 1);
    ok(&lib, &["table", "field", "rename", "书单", "页", "页数"]);
    // formulas follow a rename; one whose field goes is reported
    ok(&lib, &["table", "field", "add", "书单", "密度", "--type", "formula", "--formula", "{评分} / {页数}"]);
    ok(&lib, &["table", "field", "rename", "书单", "页数", "页码"]);
    let s = json(&lib, &["table", "书单"])["schema"].clone();
    assert_eq!(s.as_array().unwrap().iter().find(|f| f["name"] == "密度").unwrap()["formula"], "{评分} / {页码}");
    let d = json(&lib, &["table", "field", "delete", "书单", "页码", "作者"]);
    assert_eq!(d["broken"], serde_json::json!(["密度"]));
    assert_eq!(json(&lib, &["table", "书单"])["fields"], serde_json::json!(["书名", "状态", "评分", "读完", "密度"]));
}

#[test]
fn options_are_renamed_added_and_removed() {
    let lib = books();
    let c = json(&lib, &["table", "field", "change", "书单", "状态", "--rename-option", "想读=待读", "--remove-option", "读完", "--add-option", "弃读"]);
    assert_eq!(c["cells_cleared"], 2);
    let t = json(&lib, &["table", "书单"]);
    assert_eq!(col(&t, "状态"), ["在读", "", "待读", "", "待读", "在读"]);
    assert_eq!(t["schema"][2]["options"], serde_json::json!(["在读", "待读", "弃读"]));
    fails(&lib, &["table", "field", "change", "书单", "状态", "--rename-option", "没有=x"], "not_found", 3);
    fails(&lib, &["table", "field", "change", "书单", "状态", "--rename-option", "在读=待读"], "exists", 1);
    fails(&lib, &["table", "field", "change", "书单", "书名", "--add-option", "x"], "bad_args", 2);
    // --options is the whole list: cells of the dropped ones empty
    let c = json(&lib, &["table", "field", "change", "书单", "状态", "--options", "在读,新"]);
    assert_eq!(c["cells_cleared"], 2);
    assert_eq!(json(&lib, &["table", "书单"])["schema"][2]["options"], serde_json::json!(["在读", "新"]));
}

#[test]
fn views_are_added_set_renamed_and_deleted() {
    let lib = books();
    let v = json(&lib, &["table", "view", "add", "书单", "看板", "--kind", "看板"]);
    assert_eq!((v["added"]["kind"].as_str(), v["added"]["stack"].as_str()), (Some("kanban"), Some("状态")));
    fails(&lib, &["table", "view", "add", "书单", "看板"], "exists", 1);
    fails(&lib, &["table", "view", "add", "书单", "x", "--kind", "pie"], "bad_args", 2);
    ok(&lib, &["table", "view", "add", "书单", "日历", "--kind", "calendar"]);
    let s = json(&lib, &["table", "view", "set", "书单", "日历", "--date", "读完", "--end", "读完", "--where", "读完 notempty", "--order", "读完,书名", "--hide", "作者"]);
    assert_eq!((s["view"]["date"].as_str(), s["view"]["end"].as_str(), s["view"]["rows"].as_u64()), (Some("读完"), Some("读完"), Some(3)));
    assert_eq!(s["view"]["hidden"], serde_json::json!(["作者"]));
    fails(&lib, &["table", "view", "set", "书单", "日历", "--date", "书名"], "bad_args", 2);
    fails(&lib, &["table", "view", "set", "书单", "日历", "--hide", "书名"], "bad_args", 2);
    let s = json(&lib, &["table", "view", "set", "书单", "日历", "--clear", "filter", "--show", "作者", "--sort", "页数:desc"]);
    assert_eq!((s["view"].get("where"), s["view"].get("hidden"), s["view"]["sort"][0].as_str()), (None, None, Some("页数:desc")));
    let list = ok(&lib, &["table", "view", "书单"]);
    assert!(list.contains("看板 (kanban) 6 rows · stack 状态") && list.contains("日历 (calendar) 6 rows · sort 页数:desc"), "{list}");
    ok(&lib, &["table", "view", "rename", "书单", "日历", "读完日历"]);
    fails(&lib, &["table", "view", "rename", "书单", "读完日历", "看板"], "exists", 1);
    ok(&lib, &["table", "view", "delete", "书单", "表格视图"]);
    ok(&lib, &["table", "view", "delete", "书单", "看板"]);
    let e = fails(&lib, &["table", "view", "delete", "书单", "读完日历"], "bad_args", 2);
    assert!(e.contains("only view"), "{e}");
    let views = json(&lib, &["table", "view", "书单"])["views"].clone();
    assert_eq!(views.as_array().unwrap().iter().map(|v| v["name"].as_str().unwrap()).collect::<Vec<_>>(), ["读完日历"]);
}

#[test]
fn tables_of_a_file_are_added_renamed_and_removed() {
    let lib = books();
    let n = json(&lib, &["table", "new", "书单", "作者", "--fields", "姓名,国家"]);
    assert_eq!((n["fields"].clone(), n["rows"].as_u64()), (serde_json::json!(["姓名", "国家"]), Some(0)));
    assert_eq!(json(&lib, &["table", "new", "书单", "空表"])["rows"], 5, "as the app makes one");
    fails(&lib, &["table", "new", "书单", "作者"], "exists", 1);
    fails(&lib, &["table", "new", "书单", "_x"], "bad_args", 2);
    assert_eq!(json(&lib, &["table", "书单"])["tables"], serde_json::json!(["书单", "作者", "空表"]));
    ok(&lib, &["table", "rename", "书单", "空表", "草稿"]);
    fails(&lib, &["table", "rename", "书单", "草稿", "作者"], "exists", 1);
    fails(&lib, &["table", "rename", "书单", "没有", "x"], "not_found", 3);
    ok(&lib, &["table", "remove", "书单", "草稿"]);
    ok(&lib, &["table", "field", "add", "书单", "作家", "--type", "link", "--link", "作者"]);
    let e = fails(&lib, &["table", "remove", "书单", "作者"], "linked", 1);
    assert!(e.contains("linked from 书单.作家") && e.contains("--force"), "{e}");
    let r = json(&lib, &["table", "remove", "书单", "作者", "--force"]);
    assert_eq!(r["unlinked"], serde_json::json!(["书单.作家"]));
    let t = json(&lib, &["table", "书单"]);
    assert_eq!((t["tables"].clone(), t["schema"][6]["type"].as_str()), (serde_json::json!(["书单"]), Some("text")));
    let e = fails(&lib, &["table", "remove", "书单", "书单"], "bad_args", 2);
    assert!(e.contains("only table"), "{e}");
}

#[test]
fn rows_are_imported_by_column_name() {
    let lib = books();
    let file = lib.join("more.csv");
    std::fs::write(&file, "书名,页数,出版社\n三国演义,800,人民文学\n水浒传,900,\n").unwrap();
    let e = fails(&lib, &["table", "import", "书单", "--csv", file.to_str().unwrap()], "not_found", 3);
    assert!(e.contains("no field \"出版社\"") && e.contains("--create-fields"), "{e}");
    assert_eq!(json(&lib, &["table", "书单"])["total"], 6, "nothing was imported");
    let i = json(&lib, &["table", "import", "书单", "--csv", file.to_str().unwrap(), "--create-fields"]);
    assert_eq!((i["added"].as_array().unwrap().len(), i["created_fields"].clone()), (2, serde_json::json!([{ "name": "出版社", "type": "text" }])));
    let t = json(&lib, &["table", "书单", "--where", "页数 >= 800", "--fields", "书名,出版社"]);
    assert_eq!((col(&t, "书名"), col(&t, "出版社")), (vec!["红楼梦".to_string(), "平凡的世界".into(), "三国演义".into(), "水浒传".into()], vec!["".to_string(), "".into(), "人民文学".into(), "".into()]));
    // JSON rows, and CSV on standard input with a new 单选 option
    ok(&lib, &["table", "import", "书单", "--rows", r#"[{"书名":"西游记","状态":"弃读","_id":"r1"}]"#]);
    let mut child = Command::new(env!("CARGO_BIN_EXE_quiet")).env("QUIET_NO_TELEMETRY", "1").args(["--library", lib.to_str().unwrap(), "table", "import", "书单", "--csv", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all("书名\t评分\n聊斋\t8.8\n".as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let t = json(&lib, &["table", "书单", "--where", "书名 = 西游记", "--any", "--where", "书名 = 聊斋"]);
    assert_eq!((col(&t, "状态"), col(&t, "评分")), (vec!["弃读".to_string(), "".into()], vec!["".to_string(), "8.8".into()]));
    fails(&lib, &["table", "import", "书单", "--rows", r#"[{"每页":"1"}]"#, "--create-fields", "--table", "没有"], "not_found", 3);
}

#[test]
fn rows_are_updated_and_deleted_by_condition() {
    let lib = books();
    let file = lib.join("书单.sqlite");
    let before = std::fs::read(&file).unwrap();
    // a dry run lists the rows it would delete and writes nothing
    let dry = ok(&lib, &["table", "delete", "书单", "--where", "状态 = 读完", "--dry-run"]);
    assert!(dry.contains("would save 书单.sqlite") && dry.contains(" 活着, ") && dry.contains(" 围城") && dry.contains("(dry run: nothing written)"), "{dry}");
    let dry = ok(&lib, &["table", "update", "书单", "--where", "状态 = 想读", "--set", "状态=在读", "--dry-run"]);
    assert!(dry.contains("百年孤独, ") && dry.contains("红楼梦"), "{dry}");
    assert_eq!(std::fs::read(&file).unwrap(), before, "a dry run writes nothing");
    let u = json(&lib, &["table", "update", "书单", "--where", "状态 = 想读", "--set", "状态=在读"]);
    assert_eq!(u["updated"].as_array().unwrap().len(), 2);
    assert_eq!(json(&lib, &["table", "书单", "--where", "状态 = 在读"])["total"], 4);
    assert_eq!(ok(&lib, &["table", "update", "书单", "--where", "页数 > 5000", "--set", "状态=读完"]), "updated no rows: none matched\n");
    let d = json(&lib, &["table", "delete", "书单", "--where", "页数 > 1000", "--any", "--where", "评分 < 9"]);
    assert_eq!(d["rows"], serde_json::json!(d["rows"].as_array().unwrap().iter().filter(|r| ["红楼梦", "平凡的世界", "围城"].iter().any(|t| r.as_str().unwrap().ends_with(t))).collect::<Vec<_>>()));
    assert_eq!(d["deleted"], 3);
    assert_eq!(col(&json(&lib, &["table", "书单"]), "书名"), ["三体", "活着", "百年孤独"]);
    fails(&lib, &["table", "delete", "书单"], "usage", 2);
    // a script's empty condition list is refused, not taken as every row
    let o = quiet(&lib, &["run", "-e", "lib.doc('书单').deleteWhere([])"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("error[bad_args]: where: give at least one condition"));
    assert_eq!(json(&lib, &["table", "书单"])["total"], 3);
}

#[test]
fn a_file_changed_while_a_script_runs_stops_the_commit() {
    let lib = books();
    let lib_s = lib.to_str().unwrap().to_string();
    // the script reads and edits the table, then waits while the file changes under it
    let child = Command::new(env!("CARGO_BIN_EXE_quiet")).env("QUIET_NO_TELEMETRY", "1")
        .args(["--library", &lib_s, "run", "-e", "const b = lib.doc('书单'); b.addField('备注', 'text'); const t = Date.now(); while (Date.now() - t < 3000) {}"])
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1000));
    ok(&lib, &["table", "add", "书单", "--set", "书名=别人加的"]);
    let o = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(4), "{err}");
    assert!(err.contains("error[conflict]") && err.contains("书单.sqlite changed on disk"), "{err}");
    let t = json(&lib, &["table", "书单"]);
    assert!(!t["fields"].as_array().unwrap().contains(&Value::from("备注")) && t["total"] == 7, "the other write stays, the script's is not made");
}

#[test]
fn scripts_and_calls_reach_every_operation() {
    let lib = books();
    let code = r#"
const b = lib.doc('书单');
b.addField('标签', 'multi_select', { options: ['好'] });
b.changeField('状态', { renameOptions: { 想读: '待读' } });
b.addView('长篇', { kind: 'grid' });
b.setView('长篇', { where: ['页数 >= 1000'], sort: '页数:desc', stats: { 页数: 'sum' } });
b.renameView('长篇', '大部头');
b.addTable('作者', { fields: ['姓名'] });
b.renameTable('作者', '作家');
b.importRows('书名,标签\n新书,好\n');
const updated = b.updateWhere([{ field: '状态', op: '=', value: '待读' }], { 标签: '好' });
const gone = b.deleteWhere('书名 = 活着');
b.deleteFields(['作者']);
const views = b.views().map(v => `${v.name}:${v.where ?? ''}:${v.stats?.页数 ?? ''}`);
const big = b.table({ view: '大部头', fields: ['书名'] }).rows.map(r => r.书名);
const csv = b.table({ where: '标签 ~ 好', fields: '书名', as: 'csv' }).text;
b.removeTable('作家');
return { updated: updated.length, gone, views, big, csv, tables: b.table().tables };
"#;
    let o = quiet(&lib, &["run", "-e", code]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8_lossy(&o.stdout);
    let first: Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
    assert_eq!(first["updated"], 2);
    assert!(first["gone"][0].as_str().unwrap().ends_with(" 活着"));
    assert_eq!(first["views"], serde_json::json!(["表格视图::", "大部头:页数 >= 1000:sum"]));
    assert_eq!(first["big"], serde_json::json!(["红楼梦", "平凡的世界"]));
    assert_eq!(first["csv"], "书名\n百年孤独\n红楼梦\n新书\n");
    assert_eq!(first["tables"], serde_json::json!(["书单"]));
    assert!(out.contains("saved 书单.sqlite") && out.contains("+ field 标签 (multi_select)"), "{out}");
    // the same through `quiet call`, which reaches every operation by name
    let c = json(&lib, &["call", "table_field_add", r#"{"doc":"书单","name":"备注","type":"文本"}"#]);
    assert_eq!((c["added"].as_str(), c["committed"].as_bool()), (Some("备注"), Some(true)));
    let v = json(&lib, &["call", "table", r#"{"doc":"书单","where":[{"field":"页数","op":">","value":1000}],"sort":["页数:desc"],"fields":["书名"]}"#]);
    assert_eq!(col(&v, "书名"), ["红楼梦", "平凡的世界"]);
    let e = fails(&lib, &["call", "table_view_delete", r#"{"doc":"书单","view":"没有"}"#], "not_found", 3);
    assert!(e.contains("views: 表格视图, 大部头"), "{e}");
}

#[test]
fn table_commands_have_help() {
    let lib = library();
    let h = ok(&lib, &["help", "table"]);
    for want in ["--where", "--view", "--stats", "--as csv|md", "table field add", "table field change", "table view set", "table new", "table import", "--create-fields", "table update DOC --where"] { assert!(h.contains(want), "help table lacks {want}"); }
}
