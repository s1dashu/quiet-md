---
name: quiet-md-notes
description: 读写和管理 Quiet.md 本地笔记库（DocxXML 格式）。当用户需要创建、读取、列出、搜索、更新或备份 quiet.md 笔记文档，或操作 Quiet.md 文档库文件时使用。
metadata:
  version: "0.1.0"
license: MIT
---

# Quiet.md 笔记操作指南

Quiet.md 是一款极简原生的 macOS 本地笔记应用，使用飞书兼容的 DocxXML 作为底层文档格式，所有笔记以纯文件形式存储在本地。

AI 编程助手（Claude Code、Cursor、Codex、Antigravity 等）无需依赖任何私有 API 或守护进程，直接通过本地文件系统即可可靠地读取与写入 Quiet.md 笔记。

---

## 1. 文档库定位

Quiet.md 文档库位置解析顺序：
1. **环境变量**：优先检查 `$QUIET_MD_HOME`，若设置且非空则以其为准。
2. **默认路径**：未设置环境变量时，默认路径为 `~/Documents/Quiet.md/`。

目录布局：
```text
Quiet.md/
├── format.json          # {"format": 1} 目录布局版本 (store::FORMAT)
├── index.json           # 文档元数据索引 (文档 id -> Meta)
├── index.json.bak       # 上一版 index.json (崩溃自动恢复备份)
├── .lock                # flock 写锁 (写索引时短暂独占，非单实例标记)
├── docs/<id>.xml        # 每篇文档一份独立的 DocxXML 文件
├── assets/              # 图片、附件目录；assets/thumbs 为预览缩略图
└── trash/               # 回收站 (包含其专属 index.json 与 <id>.xml)
```

---

## 2. 核心操作规范

### 2.1 创建笔记（直写自动发现）

Quiet.md 支持**手放文件自动识别**：只要直接在 `docs/` 下放置合法的 `<id>.xml`，应用在启动或扫描时会自动呈现于列表顶层，无需强制修改 `index.json`。

**标准创建流程**：
1. **生成文档 ID**：生成标准小写 UUIDv4（如 `a1b2c3d4-e5f6-4a7b-8c9d-0e1f2a3b4c5d`）。
2. **构建 DocxXML 正文**：
   - **首行必须是 `<title>`**：必须包含文档标题，支持可选属性，如 `<title icon="📘" cover="covers/c1.jpg" cover-y="0.25">我的笔记</title>`。
   - **顶层块换行**：正文中的顶层 block 各占一行，不要挤在同一行。
   - **嵌套层级缩进 2 空格**：如列表、引用块、分栏等容器内部的子块每层缩进 2 个空格。
3. **安全原子写入**：
   - 遵循 Quiet.md 的原子写入原则：先写到 `docs/<id>.xml.tmp`，再 rename 为 `docs/<id>.xml`，避免应用并发读取到半成品。

### 2.2 读取与搜索笔记

1. **查找笔记**：遍历 `docs/*.xml`，读取每个文件的第一行 `<title>...</title>` 即可提取文档标题与图标。
2. **过滤状态**：若要排除已删除文档，注意只读 `docs/`，不要读 `trash/` 目录。
3. **内容匹配**：正文为纯文本 XML，可直接按行检索关键字或解析标签结构。

### 2.3 修改笔记

1. 读取原文档 `docs/<id>.xml`。
2. 保留首行 `<title>` 的图标等既有属性，仅按需调整正文块。
3. 经 `docs/<id>.xml.tmp` 原子重命名写回。

---

## 3. DocxXML 块语法速查

DocxXML 是飞书/Lark 的文档 XML 规范。以下为核心语法与避坑要点：

### 常用块与语法对照

| 块类型 | XML 语法示例 | 关键约束与说明 |
|---|---|---|
| 文档标题 | `<title icon="📝">标题</title>` | **必须是首行**；支持 `icon`、`cover`、`cover-y` |
| 普通段落 | `<p>普通 <b>粗体</b> <em>斜体</em> <code>代码</code></p>` | 行内支持 `span`（颜色）、`a`（链接）、`<latex>` |
| 标题 | `<h1>一级标题</h1>` / `<h2 folded="true">折叠标题</h2>` | 支持 `h1`~`h9`；`seq="auto"` 为自动编号；`folded="true"` 折叠 |
| 无序列表 | `<ul>\n  <li>项一\n    <ul>\n      <li>子项</li>\n    </ul>\n  </li>\n</ul>` | 嵌套子列表缩进 2 空格 |
| 空项子列表 | `<li><p></p>\n  <p>空项下的段落</p>\n</li>` | **重要**：空项有子段落必须加 `<p></p>`，否则读回会被合并 |
| 有序列表 | `<ol>\n  <li>首项</li>\n  <li seq="3">从三开始</li>\n</ol>` | `seq` 指定其实序号 |
| 任务清单 | `<checkbox done="true">已完成</checkbox>` | `done="false"` 表示未完成 |
| 代码块 | `<pre lang="rust" caption="例" wrap="true"><code>代码正文</code></pre>` | 注意：代码块标签是 `<pre><code>`，不是 `<code-block>` |
| 引用块 | `<blockquote>\n  <p>引用正文</p>\n</blockquote>` | 容器块，内部包裹 `<p>`、`<ul>` 等子块 |
| 分割线 | `<hr/>` | 单独占一行 |
| 提示块/高亮块 | `<callout emoji="💡" background-color="light-orange" border-color="orange">\n  <p>内容</p>\n</callout>` | 容器块，支持 `emoji` 与背景/边框语义色 |
| 双栏/多栏 | `<grid>\n  <column width-ratio="0.5">\n    <p>左</p>\n  </column>\n  <column width-ratio="0.5">\n    <p>右</p>\n  </column>\n</grid>` | `width-ratio` 比例总和为 1.0 |
| 图片/附件 | `<img href="assets/demo.png" width="300" caption="图" align="center"/>`<br/>本地文件：`<source path="assets/demo.zip" name="demo.zip"/>` | 图片用 `path` 或 `href`；附件文件放 `assets/` |
| 倒计时 (扩展) | `<countdown duration="300" start-time="1700000000" notify="true"/>` | Quiet.md 本地扩展块 |

---

## 4. 关键陷阱与排坑指南 (Gotchas)

1. **代码块标签格式**：DocxXML 的代码块是 `<pre lang="..."><code>...</code></pre>`，而非 Markdown 风格或 `<code-block>`。代码块内部无行内样式。
2. **表格单元格格式**：
   - 表格单元格 `<td>` 或 `<th>` 内的所有 block 必须**写在单行内，不缩进**。
   - 单元格内换行必须转义为 `&#10;`，切勿直接打换行符，以免破坏表格解析。
3. **父子嵌套约束**：
   - 只有容器类（`<blockquote>`、`<callout>`、`<grid>`、`<column>`）和支持缩进的文本块（段落、标题、列表、checkbox）允许包含子块。
   - 代码块、分割线、图片等**绝不能包含子级**。
4. **行内对象与未知标签保真**：
   - Quiet.md 读写引擎会自动原样保留不认识的 XML 标签（解析为 Raw block 或 Inline 对象）。修改已有文档时，遇到未知标签务必原样保留，不要清理。
5. **多行空项列表**：
   - 空列表项如果带有子段落，必须写为 `<li><p></p><p>子内容</p></li>`，缺少空 `<p>` 会导致读回时子段落被并进列表项。

---

## 5. 可选：更新 index.json 元数据

若需调整文档在侧栏的展示配置（如指定父子层级、修改时间、固定排序等），可修改 `index.json`。

### Meta 结构示例
```json
{
  "<uuid>": {
    "created": 1700000000,
    "modified": 1700000000,
    "opened": 0,
    "pinned": false,
    "starred": 0,
    "parent": null,
    "width": "standard",
    "reading": false
  }
}
```

### 并发与安全建议
- `index.json` 是单文件整体存储。编辑前可先检查是否持有 `.lock`。
- 修改时先读入完整 JSON，更新键值，再通过 `.tmp` 临时文件原子 rename 覆盖。

---

## 6. 深入参考资料

当需要构造复杂表格、数学公式或核对底层序列化行为时，按需查阅自包含参考文件：
- [references/all-blocks.xml](references/all-blocks.xml)：收录了所有 Block 类型规范写法的基准 fixture（往返测试基准）。
- [references/format.md](references/format.md)：关于原子写入、索引备份恢复、脏文件处理和存储限制的完整技术规范。

