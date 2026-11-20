---
name: quiet-md-pull
description: 从飞书/Lark 云文档拉取/同步到 Quiet.md 本地笔记库。当用户需要把飞书在线云文档导出、下载、同步或备份到本地 quiet.md 时使用。
metadata:
  version: "0.2.0"
  requires-bin: "lark-cli"
license: MIT
---

# 飞书云文档拉取到 Quiet.md 笔记（quiet-md-pull）

本指南指导 AI 编程助手将飞书/Lark 在线云文档（Docx / Wiki）拉取并安全落盘为 Quiet.md 本地笔记（DocxXML 格式）。

核心设计目标：**完整保持 DocxXML 块结构、下载并本地化图片附件到 `assets/`、通过原子写入将笔记写入 `docs/<uuid>.xml`，利用 quiet.md 原生文件发现机制实现即时可见**。

---

## 0. 协议防断裂指针（CRITICAL：以 lark-cli 本地 Skill 为权威）

`lark-cli` 的 CLI 参数、API 协议与参考规则可能随版本迭代升级。**严禁把参数与操作规则硬编码视为永久真理**。
在执行飞书文档拉取与素材下载前，必须按需先用 `lark-cli skills read` 检视本地绑定的权威技能规范：

- **读取云文档权威指引（含最新 fetch 参数与 --detail 说明）**：
  ```bash
  lark-cli skills read lark-doc/references/lark-doc-fetch.md
  ```
- **素材下载权威指引（图片/附件下载）**：
  ```bash
  lark-cli skills read lark-doc/references/lark-doc-media-download.md
  ```
- **CLI 原生帮助兜底**：随时运行 `lark-cli docs +fetch --help` 或 `lark-cli docs +media-download --help` 校验当前版本的最新可用参数。

---

## 1. 核心流程与原则

1. **格式选型**：
   - 提取远端文档时必须指定 `--doc-format xml`。DocxXML 与 Quiet.md 原生文档格式 100% 契合，切勿使用 Markdown 格式中转，以免丢失富文本属性、高亮块、分栏等高级格式。
2. **素材本地化**：
   - 飞书中的图片 `<img token="..."/>` 或附件 `<source token="..."/>`，需下载到 Quiet.md 文档库的 `assets/` 目录下，并重写为本地相对路径引用（如 `<img href="assets/<filename>"/>` 或 `path="assets/<filename>"`）。
3. **安全原子写入**：
   - 绝不直接向 `docs/<uuid>.xml` 写入流，统一先写入 `docs/<uuid>.xml.tmp`，完成刷盘后再 `rename` 覆盖，保证 quiet.md 永远读到完整文件。

---

## 2. 标准拉取步骤

### 步骤 1：定位本地文档库

解析 Quiet.md 文档库根目录：
- 优先环境变量 `$QUIET_MD_HOME`；
- 默认目录为 `~/Documents/Quiet.md/`。
- 确保子目录 `docs/` 与 `assets/` 存在。

### 步骤 2：读取远端 DocxXML 内容

运行 `lark-cli docs +fetch` 获取完整 XML 结构（最新参数详见 `lark-cli docs +fetch --help`）：
```bash
lark-cli docs +fetch --doc "<doc_token_or_url>" --doc-format xml --detail simple --as user
```
返回的 JSON 数据中 `data.document.content` 即为原生的 DocxXML 文本。

### 步骤 3：下载并本地化多媒体资源（如有）

遍历 DocxXML 中的图片与附件节点：
1. **下载素材**：
   - 对于带 `token` 的图片或附件，调用下载命令保存至 `assets/`（详见 `lark-cli skills read lark-doc/references/lark-doc-media-download.md`）：
     ```bash
     lark-cli docs +media-download --doc "<doc_token>" --token "<media_token>" --output "$QUIET_MD_HOME/assets/<token>.png" --as user
     ```
2. **替换 XML 路径**：
   - 将原节点更新为本地路径属性，例如：
     ```xml
     <img href="assets/<token>.png" width="300" caption="示例"/>
     ```

### 步骤 4：生成本地 UUID 并落盘

1. **确定文档 ID**：
   - 若已有本地笔记与该云文档对应，复用已有的 `<uuid>`；
   - 若为首次导入，生成全新小写 UUIDv4（如 `3c8d1e2a-4b5c-4e6f-8a9b-0c1d2e3f4a5b`）。
2. **规整 DocxXML 排版**：
   - 确保首行为 `<title>`（提取自远端内容或第一行）。
   - 顶层 block 各占一行，嵌套子级（如列表、引用）缩进 2 空格。
3. **原子写入文件**：
   - 写入临时文件：`$QUIET_MD_HOME/docs/<uuid>.xml.tmp`
   - 重命名覆盖：重命名为 `$QUIET_MD_HOME/docs/<uuid>.xml`

### 步骤 5：验证本地生效

1. 确认 `$QUIET_MD_HOME/docs/<uuid>.xml` 存在且非空；
2. Quiet.md 应用在下次激活或刷新时会自动检测到手放文件，并在侧栏顶层呈现该笔记。

---

## 3. 参考规范

- `lark-cli skills read lark-doc/references/lark-doc-fetch.md`：飞书云文档读取最新协议。
- `lark-cli skills read lark-doc/references/lark-doc-media-download.md`：飞书素材下载最新协议。
- [references/all-blocks.xml](references/all-blocks.xml)：Quiet.md 全 Block 规范写法基准。
- [references/format.md](references/format.md)：Quiet.md 文档库目录规范与原子写入。

