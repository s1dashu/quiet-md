---
name: quiet-md-push
description: 将 Quiet.md 本地笔记增量同步/推送到飞书/Lark 云文档。当用户需要把本地 quiet.md 笔记上传、更新或同步到飞书云文档，且需要保留飞书上已有评论不丢失时使用。
metadata:
  version: "0.2.0"
  requires-bin: "lark-cli"
license: MIT
---

# Quiet.md 笔记增量推送到飞书云文档（quiet-md-push）

本指南指导 AI 编程助手将 Quiet.md 本地笔记（DocxXML 格式）**增量同步/更新**至飞书/Lark 在线云文档。

核心设计目标：**严禁全量覆盖（`overwrite`），通过 Block 级增量 Patch（`block_replace` / `block_insert_after` / `block_delete`）保留飞书云文档上锚定在原有 block 上的评论、划词批注与协同记录**。

---

## 0. 协议防断裂指针（CRITICAL：以 lark-cli 本地 Skill 为权威）

`lark-cli` 的 CLI 参数、API 协议与参考规则可能随版本迭代升级。**严禁把参数与操作规则硬编码视为永久真理**。
在执行飞书文档更新前，必须按需先用 `lark-cli skills read` 检视本地绑定的权威技能规范：

- **读取云文档权威指引**：
  ```bash
  lark-cli skills read lark-doc/references/lark-doc-fetch.md
  ```
- **更新云文档权威指引（含最新 block_replace 语法与参数）**：
  ```bash
  lark-cli skills read lark-doc/references/lark-doc-update.md
  ```
- **媒体素材操作指引**：
  ```bash
  lark-cli skills read lark-doc/references/lark-doc-media-insert.md
  ```
- **CLI 原生帮助兜底**：随时运行 `lark-cli docs +update --help` 校验当前版本的最新可用参数。

---

## 1. 核心铁律与防坑红线

1. **绝对禁止 `lark-cli docs +update --command overwrite`**：
   - 全量覆盖会销毁远端全部 block ID 并重新生成，**导致远端所有历史评论、内联批注瞬时全部丢失**。
   - 必须通过局部 Block 级指令（`block_replace`、`block_insert_after`、`block_delete`、`str_replace`）推进增量更新。
2. **读远端必须带 ID**：
   - 必须先执行 `lark-cli docs +fetch --doc "<token>" --detail with-ids` 获取当前远端每一个 block 的 `block_id`。
3. **保持未变更 block 绝对不动**：
   - 比对本地 DocxXML 与远端 XML 时，未修改的段落或标题不要触发任何更新，确保其 `block_id` 原样留存。
4. **表格单行无缩进**：
   - 远端飞书与本地 DocxXML 均遵循表格单元格单行规则，换行必须写为 `&#10;`。

---

## 2. 标准增量推送流程

### 步骤 1：定位本地笔记与远端文档

1. **解析本地笔记**：
   - 文档库位置：优先检查 `$QUIET_MD_HOME`，未设置则为 `~/Documents/Quiet.md/`。
   - 读取目标笔记 `docs/<uuid>.xml`，提取首行 `<title>` 与正文 block 列表。
2. **确定远端目标**：
   - 若用户提供了飞书云文档 URL 或 token（如 `docx/` 或 `wiki/` 链接），直接以其为目标；
   - 若本地或项目维护了映射配置（如 `.sync.json`），提取对应的 `doc_token`。

### 步骤 2：读取远端最新状态（带 block_id）

运行以下命令获取远端当前结构及 ID 锚点（最新用法可随时核对 `lark-cli docs +fetch --help`）：
```bash
lark-cli docs +fetch --doc "<doc_token>" --detail with-ids --as user
```
响应中包含带 `block-id` 属性的 XML 树，例如：
```xml
<title block-id="blk_0">设计方案</title>
<p block-id="blk_1">第一段内容...</p>
<p block-id="blk_2">第二段内容...</p>
```

### 步骤 3：Block 级差异比对 (Diff)

将本地 DocxXML 的块结构与远端返回的块结构进行自上而下的逐块对齐：

1. **标题变动**：
   - 若 `<title>` 文本或图标改变，用 `str_replace` 或针对根 block 的 `block_replace` 更新标题。
2. **纯内容替换（修改段落）**：
   - 若远端 `blk_1` 内容在本地被修改：
     ```bash
     lark-cli docs +update --doc "<doc_token>" --command block_replace --block-id "blk_1" --content '<p>修改后的本地段落内容</p>' --as user
     ```
3. **新增段落/章节**：
   - 若本地在 `blk_1` 之后新增了一段，找到锚点 block ID：
     ```bash
     lark-cli docs +update --doc "<doc_token>" --command block_insert_after --block-id "blk_1" --content '<p>新插入的段落</p>' --as user
     ```
4. **删除段落**：
   - 若本地删除了远端的某段 `blk_2`：
     ```bash
     lark-cli docs +update --doc "<doc_token>" --command block_delete --block-id "blk_2" --as user
     ```
5. **连续范围替换**：
   - 若同父容器下连续多块被大幅重写，使用区间替换：
     ```bash
     lark-cli docs +update --doc "<doc_token>" --command block_replace --start-block-id "blk_start" --end-block-id "blk_end" --content '<p>新内容1</p><p>新内容2</p>' --as user
     ```

### 步骤 4：本地图片/素材上传（如有）

若本地 DocxXML 中包含指向 `assets/` 的本地图片或附件（如 `<img href="assets/demo.png"/>`）：
- 先通过 `lark-cli docs +media-insert` 将本地图片追加到远端；
- 或使用 `lark-cli docs +media-upload` 挂载到指定 block。具体可查阅 `lark-cli skills read lark-doc/references/lark-doc-media-insert.md`。

### 步骤 5：验证与确认

执行验证检查：
```bash
lark-cli docs +fetch --doc "<doc_token>" --detail simple --as user
```
核对远端内容已与本地一致，且原有评论（可在 fetch 的 `reference_map.comments` 中查看）完整保留。

---

## 3. 参考规范

- `lark-cli skills read lark-doc/references/lark-doc-update.md`：飞书云文档增量更新最新协议。
- [references/all-blocks.xml](references/all-blocks.xml)：Quiet.md 与 DocxXML 所有合法 Block 类型规范基准。
- [references/format.md](references/format.md)：Quiet.md 存储布局与原子写入标准。

