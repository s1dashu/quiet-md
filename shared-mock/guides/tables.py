#!/usr/bin/env python3
"""Make the guides' 数据表 files with the quiet CLI, in English and Chinese.

usage: python3 shared-mock/guides/tables.py path/to/quiet

  src/07a-reading-list.sqlite, src/zh/07a-reading-list.sqlite
      the example: 20 books, one field of each type a 数据表 offers (关联 and
      查找引用 reach into another table, which a file of one table has not)
  src/07b-guide-index.sqlite, src/zh/07b-guide-index.sqlite
      a table of the guides themselves: each record's 笔记 cell points at its
      guide, written `{{doc:key}}` here and made a path by build.py
  assets/tables/reading-week.sqlite, assets/tables/reading-week-zh.sqlite
      the small table embedded in the 数据表 guide (`<datatable src=…>`): its
      table and view names are written there too, keep them in step

They are binary files: build.py copies them as they are. The attachment a
book carries is a file of the guides' assets (assets/reading/).
"""
import json, shutil, sqlite3, subprocess, sys, tempfile
from pathlib import Path

here = Path(__file__).resolve().parent
assert len(sys.argv) == 2, __doc__
quiet = sys.argv[1]
lib = tempfile.mkdtemp()
subprocess.run([quiet, "ls"], env={"QUIET_MD_HOME": lib, "PATH": "/usr/bin:/bin"}, check=True, capture_output=True)  # makes it a library
def q(*args, rows=None):
    subprocess.run([quiet, "--library", lib, *args], input=json.dumps(rows, ensure_ascii=False) if rows else None, text=True, check=True, capture_output=True)

def make(out, title, view, fields, rows):
    """fields: (name, type, extra CLI args); the first is the table's title field."""
    q("new", title, "--kind", "table", "--fields", fields[0][0])
    q("table", "view", "rename", title, "表格视图", view)
    for name, ty, *extra in fields[1:]: q("table", "field", "add", title, name, "--type", ty, *extra)
    q("table", "add", title, "--rows", "-", rows=[{k: str(v) for k, v in zip([f[0] for f in fields], r) if v != ""} for r in rows])
    shutil.copyfile(Path(lib) / f"{title}.sqlite", out)

# The reading list's fields, by type; its rows give them in this order, up to Files.
TYPES = ["text", "text", "single_select", "multi_select", "rating", "progress", "text", "date", "date", "number", "formula",
         "currency", "number", "checkbox", "barcode", "url", "location", "person", "email", "phone", "attachment",
         "note", "auto_number", "created_time", "modified_time"]
def reading(out, title, view, names, statuses, money, rows):
    n = dict(zip(["book", "author", "status", "genres", "rating", "progress", "notes", "started", "finished", "pages", "left",
                  "price", "discount", "owned", "isbn", "wiki", "bought", "lent", "email", "phone", "files"], names))
    extra = {"status": ["--options", ",".join(statuses)], "rating": ["--format", "stars=5"], "pages": ["--format", "thousands"],
             "left": ["--formula", f"MAX({{{n['pages']}}} - ROUND({{{n['pages']}}} * {{{n['progress']}}}, 0), 0)", "--format", "0"],
             "price": ["--format", f"currency={money}"], "discount": ["--format", "percent"]}
    keys = list(n) + ["note", "no", "added", "updated"]
    fields = [(nm, ty, *extra.get(k, [])) for k, nm, ty in zip(keys, names, TYPES)]
    make(out, title, view, fields, [r[:10] + ("",) + r[10:] for r in rows])  # the formula column takes no value

W = "https://en.wikipedia.org/wiki/"
# book, author, status, genres, rating, progress, notes, started, finished, pages, price, discount, owned, isbn, wiki, bought at, lent to, email, phone, files
EN = [
    ("Thinking, Fast and Slow", "Daniel Kahneman", "Finished", "Psychology, Decisions", 5, 100, "Familiarity can feel like understanding: pause before deciding when the stakes are high.", "2026-01-04", "2026-02-10", 499, 18, "", "true", "9780374533557", W + "Thinking,_Fast_and_Slow", "Harbor Street Books, Boston", "Alex", "alex@example.com", "+1 555-0134", ""),
    ("The Design of Everyday Things", "Don Norman", "Finished", "Design", 4, 100, "A door that needs a sign is a design problem.", "2026-02-12", "2026-03-01", 368, 19.99, 20, "true", "9780465050659", W + "The_Design_of_Everyday_Things", "Harbor Street Books, Boston", "", "", "", ".assets/guides/reading/design-highlights.md"),
    ("The Mythical Man-Month", "Fred Brooks", "Reading", "Software", "", 35, "Next: chapter two.", "2026-11-02", "", 336, 34.99, "", "true", "9780201835953", W + "The_Mythical_Man-Month", "", "", "", "", ""),
    ("How to Take Smart Notes", "Sönke Ahrens", "Not started", "Writing, Productivity", "", "", "", "", "", 178, 15.99, "", "false", "", "", "", "", "", "", ""),
    ("Sapiens", "Yuval Noah Harari", "Finished", "History", 4, 100, "Shared stories let strangers cooperate.", "2026-03-05", "2026-04-02", 464, 22, 30, "true", "9780062316097", W + "Sapiens:_A_Brief_History_of_Humankind", "Corner Books, Portland", "Sam", "sam@example.com", "+1 555-0178", ""),
    ("Atomic Habits", "James Clear", "Finished", "Productivity, Psychology", 3, 100, "Make the habit obvious, attractive, easy and satisfying.", "2026-04-06", "2026-04-20", 320, 27, "", "true", "9780735211292", "", "Corner Books, Portland", "", "", "", ""),
    ("Deep Work", "Cal Newport", "Reading", "Productivity", "", 60, "Block two mornings a week for focus.", "2026-10-18", "", 296, 16.99, 15, "true", "9781455586691", "", "", "", "", "", ""),
    ("The Pragmatic Programmer", "David Thomas, Andrew Hunt", "Reading", "Software", "", 20, "", "2026-11-10", "", 352, 49.99, "", "true", "9780135957059", "", "", "", "", "", ""),
    ("Clean Code", "Robert C. Martin", "Gave up", "Software", 2, 40, "Useful rules, applied too strictly for my taste.", "2026-05-03", "", 464, 39.99, "", "true", "9780132350884", "", "", "", "", "", ""),
    ("Dune", "Frank Herbert", "Finished", "Fiction, Science fiction", 5, 100, "Read the appendices too.", "2026-06-01", "2026-06-28", 896, 10.99, "", "true", "9780441013593", W + "Dune_(novel)", "", "", "", "", ""),
    ("Nineteen Eighty-Four", "George Orwell", "Finished", "Fiction, Classics", 5, 100, "", "2026-07-02", "2026-07-12", 328, 9.99, "", "true", "9780451524935", W + "Nineteen_Eighty-Four", "", "", "", "", ""),
    ("To Kill a Mockingbird", "Harper Lee", "Not started", "Fiction, Classics", "", "", "", "", "", 336, 8.99, "", "false", "9780061120084", W + "To_Kill_a_Mockingbird", "", "", "", "", ""),
    ("The Little Prince", "Antoine de Saint-Exupéry", "Finished", "Fiction, Classics", 5, 100, "What is essential is invisible to the eye.", "2026-07-20", "2026-07-21", 96, 12.99, "", "true", "9780156012195", W + "The_Little_Prince", "", "", "", "", ""),
    ("Educated", "Tara Westover", "Not started", "Memoir", "", "", "Recommended by the book club.", "", "", 352, 18, "", "false", "9780399590504", W + "Educated_(memoir)", "", "", "", "", ""),
    ("The Elements of Style", "William Strunk Jr., E. B. White", "Finished", "Writing", 4, 100, "Omit needless words.", "2026-08-03", "2026-08-05", 105, 9.95, "", "true", "", W + "The_Elements_of_Style", "", "", "", "", ""),
    ("Gödel, Escher, Bach", "Douglas Hofstadter", "Reading", "Mathematics, Philosophy", "", 10, "Slow going; one dialogue a week.", "2026-09-01", "", 777, 24.99, "", "true", "", W + "G%C3%B6del,_Escher,_Bach", "", "", "", "", ""),
    ("A Brief History of Time", "Stephen Hawking", "Not started", "Science", "", "", "", "", "", 212, 18, 25, "false", "", W + "A_Brief_History_of_Time", "", "", "", "", ""),
    ("The Old Man and the Sea", "Ernest Hemingway", "Finished", "Fiction, Classics", 4, 100, "", "2026-08-15", "2026-08-17", 127, 11, "", "true", "", W + "The_Old_Man_and_the_Sea", "", "", "", "", ""),
    ("Meditations", "Marcus Aurelius", "Reading", "Philosophy", "", 45, "A few pages before bed.", "2026-10-01", "", 304, 10, "", "true", "", W + "Meditations", "", "", "", "", ""),
    ("The Art of Learning", "Josh Waitzkin", "Not started", "Psychology", "", "", "", "", "", 288, 17, "", "false", "", "", "", "", "", "", ""),
]
# The same books in their Chinese editions (the ISBN is the original edition's).
ZH = [
    ("思考，快与慢", "丹尼尔·卡尼曼", "已读完", "心理学、决策", 5, 100, "熟悉感很容易被误当成理解：事关重大时，先停一停再做决定。", "2026-01-04", "2026-02-10", 424, 69, "", "true", "9780374533557", W + "Thinking,_Fast_and_Slow", "上海 · 街角书店", "小林", "lin@example.com", "+86 100 0000 0134", ""),
    ("设计心理学", "唐纳德·诺曼", "已读完", "设计", 4, 100, "需要贴说明的门，是设计出了问题。", "2026-02-12", "2026-03-01", 312, 59, 20, "true", "9780465050659", W + "The_Design_of_Everyday_Things", "上海 · 街角书店", "", "", "", ".assets/guides/reading/设计心理学摘录.md"),
    ("人月神话", "弗雷德里克·布鲁克斯", "在读", "软件", "", 35, "下一步：读完第二章。", "2026-11-02", "", 296, 69, "", "true", "9780201835953", W + "The_Mythical_Man-Month", "", "", "", "", ""),
    ("卡片笔记写作法", "申克·阿伦斯", "未开始", "写作、效率", "", "", "", "", "", 232, 59, "", "false", "", "", "", "", "", "", ""),
    ("人类简史", "尤瓦尔·赫拉利", "已读完", "历史", 4, 100, "共同相信的故事，让陌生人也能合作。", "2026-03-05", "2026-04-02", 440, 68, 30, "true", "9780062316097", W + "Sapiens:_A_Brief_History_of_Humankind", "北京 · 胡同书房", "阿杰", "jie@example.com", "+86 100 0000 0178", ""),
    ("掌控习惯", "詹姆斯·克利尔", "已读完", "效率、心理学", 3, 100, "让习惯显而易见、有吸引力、简便易行、令人愉悦。", "2026-04-06", "2026-04-20", 264, 58, "", "true", "9780735211292", "", "北京 · 胡同书房", "", "", "", ""),
    ("深度工作", "卡尔·纽波特", "在读", "效率", "", 60, "每周留两个上午专心做事。", "2026-10-18", "", 240, 49, 15, "true", "9781455586691", "", "", "", "", "", ""),
    ("程序员修炼之道", "戴维·托马斯、安德鲁·亨特", "在读", "软件", "", 20, "", "2026-11-10", "", 336, 89, "", "true", "9780135957059", "", "", "", "", "", ""),
    ("代码整洁之道", "罗伯特·C. 马丁", "放弃", "软件", 2, 40, "规则有用，只是对我来说太教条。", "2026-05-03", "", 388, 59, "", "true", "9780132350884", "", "", "", "", "", ""),
    ("沙丘", "弗兰克·赫伯特", "已读完", "小说、科幻", 5, 100, "附录也值得读。", "2026-06-01", "2026-06-28", 680, 78, "", "true", "9780441013593", W + "Dune_(novel)", "", "", "", "", ""),
    ("一九八四", "乔治·奥威尔", "已读完", "小说、经典", 5, 100, "", "2026-07-02", "2026-07-12", 304, 39, "", "true", "9780451524935", W + "Nineteen_Eighty-Four", "", "", "", "", ""),
    ("杀死一只知更鸟", "哈珀·李", "未开始", "小说、经典", "", "", "", "", "", 384, 48, "", "false", "9780061120084", W + "To_Kill_a_Mockingbird", "", "", "", "", ""),
    ("小王子", "安托万·德·圣-埃克苏佩里", "已读完", "小说、经典", 5, 100, "真正重要的东西，用眼睛是看不见的。", "2026-07-20", "2026-07-21", 112, 32, "", "true", "9780156012195", W + "The_Little_Prince", "", "", "", "", ""),
    ("你当像鸟飞往你的山", "塔拉·韦斯特弗", "未开始", "回忆录", "", "", "读书会推荐。", "", "", 376, 59, "", "false", "9780399590504", W + "Educated_(memoir)", "", "", "", "", ""),
    ("风格的要素", "威廉·斯特伦克、E. B. 怀特", "已读完", "写作", 4, 100, "删去不必要的词。", "2026-08-03", "2026-08-05", 128, 36, "", "true", "", W + "The_Elements_of_Style", "", "", "", "", ""),
    ("哥德尔、艾舍尔、巴赫", "侯世达", "在读", "数学、哲学", "", 10, "读得慢，一周一篇对话。", "2026-09-01", "", 1040, 168, "", "true", "", W + "G%C3%B6del,_Escher,_Bach", "", "", "", "", ""),
    ("时间简史", "史蒂芬·霍金", "未开始", "科学", "", "", "", "", "", 256, 45, 25, "false", "", W + "A_Brief_History_of_Time", "", "", "", "", ""),
    ("老人与海", "欧内斯特·海明威", "已读完", "小说、经典", 4, 100, "", "2026-08-15", "2026-08-17", 144, 29, "", "true", "", W + "The_Old_Man_and_the_Sea", "", "", "", "", ""),
    ("沉思录", "马可·奥勒留", "在读", "哲学", "", 45, "睡前读几页。", "2026-10-01", "", 232, 38, "", "true", "", W + "Meditations", "", "", "", "", ""),
    ("学习之道", "乔希·维茨金", "未开始", "心理学", "", "", "", "", "", 256, 49, "", "false", "", "", "", "", "", "", ""),
]
reading(here / "src" / "07a-reading-list.sqlite", "Reading list", "All books",
        ["Book", "Author", "Status", "Genres", "Rating", "Progress", "Notes", "Started", "Finished", "Pages", "Pages left",
         "Price", "Discount", "Owned", "ISBN", "Wikipedia", "Bought at", "Lent to", "Email", "Phone", "Files",
         "Reading notes", "No.", "Added", "Last modified"],
        ["Not started", "Reading", "Finished", "Gave up"], "$", EN)
reading(here / "src" / "zh" / "07a-reading-list.sqlite", "阅读清单", "全部",
        ["书名", "作者", "状态", "类别", "评分", "进度", "笔记", "开始", "读完", "页数", "剩余页数",
         "价格", "折扣", "已购买", "ISBN", "维基百科", "购于", "借给", "Email", "电话", "附件",
         "读书笔记", "编号", "添加时间", "最后更新时间"],
        ["未开始", "在读", "已读完", "放弃"], "¥", ZH)

# The guides as records, their 笔记 field second: a key of build.py's KEYS per record.
def index(out, title, view, names, folders, kinds, rows):
    fields = [(names[0], "text"), (names[1], "note"), (names[2], "single_select", "--options", ",".join(folders)),
              (names[3], "single_select", "--options", ",".join(kinds)), (names[4], "number"), (names[5], "number"),
              (names[6], "checkbox"), (names[7], "multi_select")]
    make(out, title, view, fields, [(r[0], "") + r[2:] for r in rows])
    db = sqlite3.connect(out)
    for (rid, topic), r in zip(db.execute(f'SELECT id, "{names[0]}" FROM "{title}" ORDER BY _order, id').fetchall(), rows):
        assert topic == r[0]
        db.execute(f'UPDATE "{title}" SET "{names[1]}" = ? WHERE id = ?', ("{{doc:%s}}" % r[1], rid))
    # wide enough for a guide's title
    ids = dict(db.execute("SELECT name, id FROM _fields").fetchall())
    (vid, config), = db.execute("SELECT id, config FROM _views").fetchall()
    config = json.loads(config)
    config["widths"] = {str(ids[n]): w for n, w in zip(names, [160, 320, 140, 130, 90, 90, 80, 200])}
    db.execute("UPDATE _views SET config = ? WHERE id = ?", (json.dumps(config, ensure_ascii=False, separators=(",", ":")), vid))
    db.commit()
    db.close()
# topic, guide, folder, type, order, minutes, read, covers
INDEX_EN = [
    ("Start here", "welcome", "Home", "Document", 1, 4, "true", "Basics"),
    ("Writing", "writing", "Documents", "Document", 2, 9, "", "Text"),
    ("Tables and code", "tables", "Documents", "Document", 3, 8, "", "Tables, Code"),
    ("Images and files", "media", "Documents", "Document", 4, 3, "", "Images, Files"),
    ("Markdown", "markdown", "Markdown files", "Document", 5, 4, "", "Markdown"),
    ("A Markdown file", "md-example", "Markdown files", "Markdown file", 6, 1, "", "Markdown"),
    ("Boards", "boards", "Boards", "Document", 7, 8, "", "Boards"),
    ("Databases", "data-tables", "Databases", "Document", 8, 3, "", "Databases"),
    ("Organize and search", "organize", "Everyday use", "Document", 9, 7, "", "Folders, Search"),
    ("Import and back up", "data", "Everyday use", "Document", 10, 5, "", "Import, Backup"),
    ("Themes and shortcuts", "personalize", "Everyday use", "Document", 11, 4, "", "Themes, Shortcuts"),
]
INDEX_ZH = [
    ("从这里开始", "welcome", "主页", "文档", 1, 4, "true", "入门"),
    ("书写", "writing", "文档", "文档", 2, 9, "", "文字"),
    ("表格与代码", "tables", "文档", "文档", 3, 8, "", "表格、代码"),
    ("图片与附件", "media", "文档", "文档", 4, 3, "", "图片、附件"),
    ("Markdown", "markdown", "Markdown 文件", "文档", 5, 4, "", "Markdown"),
    ("一个 Markdown 文件", "md-example", "Markdown 文件", "Markdown 文件", 6, 1, "", "Markdown"),
    ("画板", "boards", "画板", "文档", 7, 8, "", "画板"),
    ("数据库", "data-tables", "数据库", "文档", 8, 3, "", "数据库"),
    ("整理与查找", "organize", "日常使用", "文档", 9, 7, "", "文件夹、搜索"),
    ("导入与备份", "data", "日常使用", "文档", 10, 5, "", "导入、备份"),
    ("主题与快捷键", "personalize", "日常使用", "文档", 11, 4, "", "主题、快捷键"),
]
index(here / "src" / "07b-guide-index.sqlite", "Guide index", "All guides",
      ["Topic", "Guide", "Folder", "Type", "Order", "Minutes", "Read", "Covers"],
      ["Home", "Documents", "Markdown files", "Boards", "Databases", "Everyday use"], ["Document", "Markdown file"], INDEX_EN)
index(here / "src" / "zh" / "07b-guide-index.sqlite", "指南目录", "全部",
      ["主题", "指南", "文件夹", "类型", "顺序", "分钟", "已读", "内容"],
      ["主页", "文档", "Markdown 文件", "画板", "数据库", "日常使用"], ["文档", "Markdown 文件"], INDEX_ZH)

# The small table the 数据表 guide shows inside its page.
make(here / "assets" / "tables" / "reading-week.sqlite", "This week", "All sessions",
     [("Book", "text"), ("Day", "date"), ("Minutes", "number"), ("Mood", "single_select", "--options", "Focused,Calm,Tired")],
     [("The Mythical Man-Month", "2026-11-23", 40, "Focused"), ("Deep Work", "2026-11-24", 25, "Tired"),
      ("Meditations", "2026-11-25", 15, "Calm"), ("Gödel, Escher, Bach", "2026-11-26", 50, "Focused")])
make(here / "assets" / "tables" / "reading-week-zh.sqlite", "本周阅读", "全部",
     [("书名", "text"), ("日期", "date"), ("分钟", "number"), ("状态", "single_select", "--options", "专注,平静,疲惫")],
     [("人月神话", "2026-11-23", 40, "专注"), ("深度工作", "2026-11-24", 25, "疲惫"),
      ("沉思录", "2026-11-25", 15, "平静"), ("哥德尔、艾舍尔、巴赫", "2026-11-26", 50, "专注")])
shutil.rmtree(lib)
