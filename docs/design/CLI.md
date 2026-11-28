# quiet CLI：设计与路线图

目标：让 Quiet.md 的文档库成为人和 Agent 共用的知识库。人用应用，Agent 用 `quiet`。两边读写同一批普通文件，互不打断。实现以 `crates/cli` 的代码为准，和本文对不上时以代码为准。

## 原则

1. **文件是底座，CLI 是上限。** 文档库本身就是 Agent 能直接读的普通文件夹（`.xml`、`.md`、`.excalidraw`、`.sqlite`）。CLI 不替代读文件，它提供的是文件做不到的事：
   - 按结构定位块
   - 改写时保持格式合法
   - 移动时改写链接
   - 并发保护
   - 跨文档一次事务
2. **两层入口，同一套实现。**
   - shell 命令做单步操作。
   - `quiet run` 在沙箱 QuickJS 里跑脚本，做循环、筛选和多步操作。
   - 两者都是 `Session::call(op, json)`。`quiet call` 和 `quiet mcp` 也走它。新能力只加一处。
3. **Agent 能自己纠错。**
   - 错误都带稳定的 code、message 和 hint。
   - 退出码分类：2 用法，3 找不到，4 版本冲突，1 其他。
   - 写操作支持 `--dry-run`（只出 diff）和 `--expect-rev`。
   - 脚本失败就什么都不写。
4. **输出省 token。** 默认输出紧凑文本，`--json` 给全字段。长文档先给大纲，读的时候按章节取。
5. **不和应用抢。**
   - 写入走引擎同一套 `Store`：文档库写锁、原子写、版本历史快照、链接改写。
   - 应用开着时，能自动接收外部改动的就接收。接收不了的，冲突要能看见，不能静默覆盖。
6. **GUI 能做的，CLI 都能做**（用户 2026-11-26 定）。用户在应用里能做的每个操作，CLI 都要能做同样的动作，功能才算闭环：管理侧栏里的文件夹、在应用里打开并定位到某一块，也算在内。新增 GUI 操作时同时想好对应的 CLI 命令；暂时没有的，记在下面「GUI 操作 → CLI 命令」表里。

## 现状

- **文档库在哪：** 和应用一样，是这台电脑上应用打开的全部顶层文件夹，也就是主页（唯一的根）下的文件夹，见 [workspaces.md](workspaces.md)。CLI 用 core 的 `TopFolders::entries()` 读同一份 `folders.json`（设了 `QUIET_MD_HOME` 就只有那一个文件夹，测试用 `QUIET_MD_FOLDERS` 指向临时的列表），顺序也一样；只有 `quiet folders add|remove` 写它（见下面「和应用联动」）。每个顶层文件夹用 `Store::open_for_tools` 打开：文件夹不在了就标为不可用，不会新建；崩溃留下的副本留给应用恢复。
- **路径从主页算起**（用户 2026-11-24 定：只有主页是根，顶层文件夹就是普通文件夹，所以没有专门挑顶层文件夹的参数）：
  - 路径第一段是顶层文件夹名（不分大小写），如 `Quiet.md/工作/周报`；也接受磁盘上的绝对路径。CLI 输出的所有路径都是这种从主页算起的路径。
  - `quiet ls` 不带参数列出主页下的顶层文件夹：名字、磁盘位置、key、文档数，或打不开的原因（`--json` 里 `top: true`）。`quiet ls -r` 列出全部。增删顶层文件夹用 `quiet folders add|remove`。
  - 两个顶层文件夹重名：第一段报 `ambiguous`，列出两边的磁盘路径和 key；用 key 当第一段就能指定。
  - 只有一个文档的路径或标题，可以不带顶层文件夹；多个文档同名报 `ambiguous`，列出各自从主页算起的路径。按 id 直接定位（id 跨文件夹唯一，W14）。
  - 新建、建文件夹、导入的落点：`--in` 指的文件夹，否则第一个能打开的顶层文件夹（和应用一致）；输出的路径写明落在哪里。`--in` 的第一段不是顶层文件夹名时，按文件夹路径或名字在所有顶层文件夹里找，唯一才用。
  - 回收站、版本历史按顶层文件夹各自处理；`untrash` 放回原处。`mv` 只能在同一个顶层文件夹里移动，移到另一个报 `cross_folder`；移到主页本身报 `bad_args`。
  - `--library DIR`：只操作这一个文件夹，路径从它里面算起，不带名字。
  - 一个顶层文件夹都没有：`no_library`。
  - 写入走各顶层文件夹自己的 `Store`（各自的写锁），CLI 不另加全局锁。
  - **跨文件夹链接**：`[[…]]` 和指向库内文件的链接都经 `Session::find_linked` 解析（`insert`、`replace`、`set`、`new --md`、`links`、`quiet run`、`quiet mcp` 共用）：
    - 先按 id 找，在哪个顶层文件夹都行；
    - 再在当前文档所在的顶层文件夹里按路径或标题找（新建文档以落点为准），这一步有多个同名照旧报 `ambiguous`；
    - 当前文件夹里没有，再到其他可用的顶层文件夹里找；`[[顶层文件夹/路径]]` 直接指那一个。每个文件夹各自匹配：只有一个文档匹配就用它；匹配落在多个文件夹里，或一个文件夹里有多个，就报 `ambiguous`，列出每个候选的路径、所在文件夹和 id，提示写成 `[[id]]`。不会替用户挑一个。
    - 按 Markdown 读出时，提及写回 `[[去掉扩展名的路径]]`；从当前文档所在的顶层文件夹解析回去会指向别的文档时，写带扩展名的路径，还不行就写 `[[id]]`。这样读出的内容能原样写回（`Session::wiki_name`）。
- **读：**
  - `ls`、`find`（多词、排序）、`outline`、`read`（md/text/xml/json；可按 ref、章节或去掉标题的章节读；默认上限 40k 字符）
  - `blocks`（按类型、文字、完成状态筛选）、`links`、`check`
- **写：**
  - 文档和文件夹：`new`（文档、Markdown、画板、数据表）、`mkdir`、`rename`、`mv`、`rm`
  - 块：`insert`、`replace`、`delete`、`move`、`set`
  - 新内容里的 `[[…]]` 会成为 @提及。
- **数据表：** `table`（按视图、条件、排序、分组读，统计，导出 csv/md），`table add|update|delete|import`，字段 `table field …`，视图 `table view …`，多表 `table new|rename|remove`。见下面「数据表」一节。
- **画板：** `board`，以及 `board add|rm`（Excalidraw 骨架；箭头写两端 id 就能自动连线）。
- **撤销和导入：** `history`、`restore`、`trash`、`untrash`、`import`。
- **和应用联动：** `folders`（列出、`add`、`remove` 顶层文件夹）、`open`（在应用里打开文档并定位到块，或打开文件夹页、回收站），`new --open`。见下一节。
- **脚本：**
  - `quiet run`：沙箱，60 秒超时，512MB 内存。整个脚本是一次事务。
  - 脚本的 `lib` API 和命令一一对应。
- **给 Agent：**
  - 配套 skill 在仓库根的 `skills/quiet-md/`（`SKILL.md` 加 `references/guide.md`），用社区通用的 skills CLI 安装：`npx skills add s1dashu/quiet-md`（Claude Code 装到 `~/.claude/skills/`，Codex 装到 `~/.agents/skills/`）。`references/guide.md` 也编进二进制，`quiet guide` 输出它，是唯一来源；原来的 `quiet skill install` 删掉了。SKILL.md 先教 Agent 检查和安装 CLI（见下面「分发」），再教用法，不提 MCP。只有一个 skill，不按功能拆分：触发描述覆盖「笔记、笔记库、周报、待办」等说法，正文是核心用法和在别人库里干活的规矩（先读后写、少改多加、批量改先 dry-run、写完告诉用户改了哪些路径、改错用 history 撤销）。
  - `quiet guide`（完整指南，`quiet skill` 是它的别名）和 `quiet help CMD`
  - `quiet mcp`（两个工具）
- **和应用共存：**
  - 共用 `Store` 的写锁。
  - 每次保存都存版本快照。
  - 应用里打开的同一篇文档，编辑器干净时自动载入 CLI 写入的内容。

代码在 `crates/cli/src`，各文件的分工：

| 文件 | 内容 |
| --- | --- |
| `session.rs` | 文档、块和提交 |
| `tables.rs` | 数据表：打开、读（查询、统计、导出）、行的增改删 |
| `table_schema.rs` | 数据表：字段、视图、多表、导入 |
| `boards.rs` | 画板 |
| `library.rs` | 版本历史、回收站、导入 |
| `js.rs`、`prelude.js` | 脚本 |
| `mcp.rs` | MCP 服务 |
| `app_control.rs` | 和应用联动：`folders`、`open` |
| `help.rs` | 每个命令的帮助 |
| 仓库根 `skills/quiet-md/` | 配套 skill；`references/guide.md` 即 `quiet guide` 的指南，编进二进制 |
| `usage.rs` | 匿名用量统计 |

测试在 `crates/cli/tests/cli.rs`，数据表的在 `crates/cli/tests/tables.rs`。

## 和应用联动：folders 与 open

用户 2026-11-26：「CLI 要能在 Quiet 里添加文件夹，能做路径操作，能打开文档并定位到某个位置。」

**`quiet folders`**（`app_control.rs`）
- 不带参数列出顶层文件夹：名字、磁盘路径、key，新建默认落到的那个标 `default`，打不开的写原因。
- `add PATH`：等同侧栏的「添加文件夹」，用 core 同一份 `TopFolders::check` / `TopFolders::add` 写 `folders.json`：原位打开、不复制，在里面建 `.quiet/`。已经打开的报 `already_open`；磁盘根和主目录和应用一样拒绝（错误码 `too_broad`，提示分别添加文稿、桌面等文件夹）。在已打开的文件夹里面、或包含已打开的文件夹都可以添加（嵌套，见 workspaces.md 2.6）。应用遇到代码项目或文件很多的文件夹会先问（`shown::survey`），CLI 报 `confirm`，加 `--force` 才添加。系统文件夹（/System、/Library、/usr 等）也和应用一样拒绝（`AddError::System`，同样报 `too_broad`，提示添加存放笔记的文件夹）。
- `remove NAME|PATH|KEY`：等同「从侧栏移除」，只改列表，磁盘上的文件和 `.quiet/` 原样保留。
- 两者都支持 `--dry-run`。设了 `QUIET_MD_HOME`（列表固定）时报 `fixed`。
- **应用跟着变：** `local_folders::follow_folder_list` 每 2 秒在后台读一次 `folders.json`，和内存里的列表按 key 比较（`TopFolders::changed_elsewhere`）。多出来的按文件里的 key 打开（`TopFolders::adopt`，不重写列表）；少了的，每个窗口先放下里面打开的文档（未保存的先存，没存完就等下一轮），再从列表移除。新加的按「添加文件夹」同样的方式在后台列出。应用自己写的列表和内存一致，不会触发。选文件轮询而不是 URL 通知：应用没开时 CLI 照样能改，下次启动读到；应用开着时不依赖系统转发。
- 已知限制：CLI 和应用在同一个 2 秒窗口里各自改列表，后写的一方会覆盖先写的一方（应用没有合并文件里的改动）。

**`quiet open`**
- `quiet open DOC[#REF]`：CLI 先在磁盘上解析文档和块（REF 和 `read` 的一样：`#3~k7f2`、`#2.1`、标题文字），把块换成带指纹的位置 ref，拼成 `quietmd://open?top=<key>&doc=<id>&block=%233~k7f2`，交给系统打开（macOS `open`，Windows `rundll32 url.dll,FileProtocolHandler`（`start` 是 cmd 内建命令，链接里的 `&` 会被当成命令分隔符），Linux `xdg-open`）；应用没开就会被启动。`--dry-run` 只打印链接。
- `quiet open --folder FOLDER`：`quietmd://open?top=<key>&folder=<id>`，打开文件夹页；只给顶层文件夹时打开它自己的页（`quiet open 工作` 不是文档时也按文件夹处理）。`quiet open --trash`：`quietmd://open?view=trash`。`quiet new … --open` 写完后打开新文档。
- `--library DIR` 时，那个文件夹要在应用的列表里（按路径找应用用的 key），否则报 `not_open`。
- 测试设 `QUIET_MD_OPEN_LOG=<文件>`：链接追加写进这个文件，不交给系统（各平台一样）。
- **链接格式和安全**（`fdl_core::app_url`）：只有 `open` 一种动作，只做导航，不写、不执行任何东西。参数只认 `top`、`doc`、`block`、`folder`、`view`，每个最多一次；`top`、`doc`、`folder` 只能是 id 字符（字母数字和 `._:-`，最长 200）；`block` 必须是 ref 格式；`view` 只认 `trash`；链接最长 2048。有任何一项不对，整条拒绝，应用提示「无法打开链接」。找不到文档或文件夹（被移走、文件夹不在侧栏）给出提示，不崩。
- **应用端**（`local_files.rs`）：macOS 的 `on_open_urls`，Windows / Linux 的命令行参数，都进 `Incoming`；等窗口列完文件夹再处理链接（`Workspace::open_link`）。文档按 id 找（跨文件夹唯一），`try_open` 打开后用 `block_ref::locate` 找块：位置上的块指纹对得上就用它，否则用全文唯一同指纹的块（块被挪过），再找不到就打开文档并提示「找不到那个块」。找到的块滚到可见（`scroll_to_block`）并进入块选中（`select_block`），和反向链接跳转同一套。块的 ref 规则从 CLI 挪进了 core 的 `block_ref.rs`，两边共用。
- **注册 scheme：**
  - macOS：只有 `scripts/release.sh` 生成的正式包在 Info.plist 里声明 `CFBundleURLTypes`（`quietmd`）。`scripts/preview.sh` 的预览包不声明（2026-11-27 定）：预览包和正式包同时存在时，链接只会交给正式版，`quiet open` 不会把用户带到一个开发版上。实测（LaunchServices 里注册两个按这两份 Info.plist 做的空壳包，问 `NSWorkspace URLsForApplicationsToOpenURL`）：候选只有正式包；换成改之前的预览 plist，两个都是候选。旧的预览包重新跑一次 `preview.sh` 就会被重新注册，不再认领 `quietmd`。
  - Windows：`packaging/windows/quiet-md.iss` 在 `HKA\Software\Classes\quietmd` 下注册 `URL Protocol`，命令是 `"quiet_md.exe" "%1"`，卸载时删除。验证：装好后在 PowerShell 跑 `Start-Process 'quietmd://open?view=trash'`，或 `quiet open --trash`，应用应打开回收站；`reg query HKCU\Software\Classes\quietmd\shell\open\command`（全局安装时是 HKLM）能看到命令。
  - Linux：`.desktop` 的 `MimeType` 加 `x-scheme-handler/quietmd`，`Exec` 改成 `%U`（文件以 `file://` 链接传入，`Incoming::from_args` 也接受）。验证：装好 .desktop 后 `xdg-mime query default x-scheme-handler/quietmd` 应返回 `com.s1dashu.quietmd.desktop`（没有就 `xdg-mime default com.s1dashu.quietmd.desktop x-scheme-handler/quietmd`），再 `xdg-open 'quietmd://open?view=trash'`。AppImage 要由 appimaged 之类的工具把 .desktop 装进系统才会注册。
  - **开发时在 macOS 上手动测 URL scheme：**
    - 只测应用怎么处理链接、不经过系统：harness 脚本里的 `url quietmd://…` 步骤（`tools/harness/README.md`），和系统转交走同一个 `Workspace::open_link`。`quiet open … --dry-run` 打印出 CLI 生成的链接。
    - 连系统转交一起测（推荐，用测试文档库）：`QUIET_MD_HOME=<临时目录> scripts/preview.sh` 起预览包，然后临时给它加上 scheme 并注册：
      ```sh
      app="dist-preview/Quiet.md Preview.app"; pb=/usr/libexec/PlistBuddy; ls=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
      $pb -c 'Add :CFBundleURLTypes array' -c 'Add :CFBundleURLTypes:0 dict' -c 'Add :CFBundleURLTypes:0:CFBundleURLSchemes array' -c 'Add :CFBundleURLTypes:0:CFBundleURLSchemes:0 string quietmd' "$app/Contents/Info.plist"
      codesign --force --sign - "$app"; $ls -f "$app"
      quiet open --trash        # 或 open 'quietmd://open?view=trash'
      ```
      测完重新跑一次 `scripts/preview.sh`（重写 Info.plist 并重新注册），预览包就不再认领 `quietmd`。同一台机器上还装着正式版时，两个都声明了 `quietmd`，系统挑哪个不固定（和是否在运行无关），在没装正式版的机器上测最确定。
    - 用正式包测：`SIGNING_IDENTITY=- scripts/release.sh` 生成一个 ad-hoc 签名、不公证的正式包（`dist-release.*/quiet.md.app`），`lsregister -f` 注册后再 `quiet open …`。注意它就是正式版：从系统启动时没有 `QUIET_MD_HOME`，打开的是默认文档库。测完 `lsregister -u` 并删掉这个目录，链接回到装好的 Quiet.md。装着正式版时同样有两个候选。
  - 应用没有单实例：Windows 和 Linux 上，应用开着时再打开链接会另起一个进程（macOS 由系统交给正在运行的那个）。做单实例转发之前，这两个平台上的 `quiet open` 会多开一个窗口。

## GUI 操作 → CLI 命令

按原则 6 维护：「已有」「本次补上」（2026-11-27，cli-app-control）、「缺」（小：调一个现成的 `Store` 方法就行；大：要先挪代码或另做设计）、「只属于界面」（窗口、视图和外观状态，CLI 不做）。

| 区域 | GUI 操作 | CLI |
| --- | --- | --- |
| 侧栏 | 浏览树、展开 | 已有 `quiet ls [F] [-r]`（按路径排，不按侧栏的手动顺序） |
| 侧栏 | 添加文件夹…（⌘⇧O）、从其他应用导入 › Obsidian 原位打开 | 本次补上 `quiet folders add PATH [--force]` |
| 侧栏 | 从侧栏移除 | 本次补上 `quiet folders remove NAME\|PATH\|KEY` |
| 侧栏 | 列出顶层文件夹 | 本次补上 `quiet folders`（`quiet ls` 也列） |
| 侧栏 | 重命名顶层文件夹 | 缺（小：`TopFolders::rename`） |
| 侧栏 | 拖拽移动文档 | 已有 `quiet mv DOC FOLDER`（同一个顶层文件夹内） |
| 侧栏 | 拖拽重排（手动排序） | 缺（小：`Store::move_in_tree`；`ls` 也要改成按 `Meta::place` 排） |
| 侧栏 | 从访达拖文件进来 | 已有 `quiet import FILE… --in F` |
| 侧栏 / 文档菜单 | 添加到「置顶」/ 从「置顶」移除 | 本次补上 `quiet pin\|unpin DOC` |
| 文档菜单 | 创建副本 | 本次补上 `quiet duplicate DOC [TITLE]`（文档；文件夹见下） |
| 文档菜单 | 打开、定位到块；从搜索结果或反向链接跳到某块 | 本次补上 `quiet open DOC[#REF]`（滚到可见并选中） |
| 文档菜单 | 在新窗口中打开 | 缺（小：`Target` 加「新窗口」） |
| 文档菜单 | 移动到 | 已有 `quiet mv` |
| 文档菜单 | 重命名、删除（进回收站） | 已有 `quiet rename`、`quiet rm` |
| 文档菜单 | 复制为 Markdown（⌘⇧C） | 已有 `quiet read DOC[#REF] --as md` |
| 文档菜单 | 下载 › Markdown（带图片） | 缺（小：`markdown::export` 加复制图片，照 transfer.rs） |
| 文档菜单 | 下载 › PDF、打印 | 缺（大：`html.rs`、`printing` 在界面 crate，依赖 WebView） |
| 文档菜单 | 文档信息（字数、创建时间、位置） | 部分：`outline` 有大小和 rev，`ls` 有修改时间；缺字数和磁盘路径（小） |
| 文档菜单 | 版本历史：看、比较、恢复 | 已有 `quiet history`、`quiet restore` |
| 版本历史 | 保存当前版本、给版本命名、另存副本、删除版本 | 缺（小：`save_version_now`、`set_version_label`、`version_document`、`delete_version`） |
| 文档菜单 | 查找和替换 | 部分：`blocks --grep` 加 `set`/`replace`，或 `quiet run`；缺整篇替换命令（中） |
| 文档菜单 | 页宽、阅读/编辑模式、打字机、段落聚焦、专注、置顶窗口、显示引用列表、快捷键 | 只属于界面（引用列表的内容 `quiet links` 能看到） |
| 文档 | 文档图标、封面 | 缺（小：`Document.icon` / `cover`，封面图片经 `import_attachment`） |
| 文档 | 编辑正文：插入、转换、移动块、待办 | 已有 `insert`/`replace`/`delete`/`move`/`set` |
| 文档 | 对齐、颜色等格式 | 缺（中：要设计参数） |
| 新建菜单 | 文档、Markdown、数据表、画板 | 已有 `quiet new T [--kind md\|table\|board]` |
| 新建菜单 | 文件夹 | 已有 `quiet mkdir N --in F` |
| 新建菜单 | 在某文件夹里新建并打开 | 本次补上 `quiet new T --in F --open` |
| 新建菜单 | 新建子文档 | 缺（小：`Store::create_child`，可做 `--child-of DOC`） |
| 新建菜单 | 快速笔记窗口（⌘⇧N） | 只属于界面（内容可以 `quiet new` 到快速笔记文件夹） |
| 文件夹页 | 打开文件夹页、主页上的顶层文件夹 | 本次补上 `quiet open --folder F` |
| 文件夹页 | 文件夹重命名、移动到、删除 | 缺（小：`Store::rename` / `set_parent` / `delete` 都支持文件夹，CLI 的 `find_entry` 只找文档） |
| 文件夹页 | 文件夹创建副本 | 缺（小：`Store::duplicate` 支持文件夹，同上） |
| 文件夹页 | 批量管理：创建副本、移动到、删除 | 部分：`quiet run` 里逐个做；文件夹同上 |
| 文件夹页 | 宫格/列表、排序方式、文件夹优先 | 只属于界面 |
| 文件夹页 / 侧栏 | 在访达中显示 | 只属于界面（路径 `quiet folders`、`ls -r` 能看到） |
| 搜索 | 搜索（⌘J） | 已有 `quiet find` |
| 导入 | 导入 Markdown / DocxXML / CSV | 已有 `quiet import` |
| 导入 | 导入文件夹、Obsidian 导入副本、Typora、思源 | 缺（大：`transfer.rs` 的导入在界面 crate，先挪进 core） |
| 导入 | 打开 Markdown 文件…（⌘O，原位编辑） | 缺（中：`Store::open_local`，`Target` 加文件路径） |
| 回收站 | 打开回收站 | 本次补上 `quiet open --trash` |
| 回收站 | 列出、恢复 | 已有 `quiet trash`、`quiet untrash` |
| 回收站 | 彻底删除、清空回收站 | 缺（小：`Store::purge` / `purge_many`；不可恢复，要定 `--force` 之类的确认方式） |
| 回收站 | 自动删除天数 | 缺（小：`Settings::set_trash_days`） |
| 设置 › 文档库 | 文件夹列表 | 本次补上 `quiet folders` |
| 设置 › 文档库 | 快速笔记存到哪个文件夹、每篇保留的版本数、清理未使用的资源 | 缺（小；`quiet check` 只列出未使用资源） |
| 设置 › 通用 | 匿名使用统计 | 已有 `quiet telemetry` |
| 设置 › 关于 | 导入最新的开始指南 | 已有 `quiet guide import [--in F] [--lang zh\|en]`（2026-11-28，guide-import）：和应用同一个 `Store::import_guides`，放进顶层文件夹 F 的顶层（默认首启放指南的那个，否则第一个），语言默认跟应用的界面语言 |
| 设置 › 通用 / 外观 / 快捷键 / 关于 | 语言、主题、字体、动画、快捷键、检查更新… | 只属于界面 |
| 设置 › 连接 Agent | 安装 skill 和命令行工具 | 不需要（这是装 CLI 本身） |
| 文件操作撤销 | ⌘Z / ⌘⇧Z 撤销移动、删除 | 缺（大：依赖窗口里的撤销栈；CLI 用 `--dry-run` 和版本历史） |
| 数据表 | 行、字段、视图、筛选排序分组、统计、导入导出 | 已有 `quiet table …`；缺复制记录、字段、视图（小） |
| 画板 | 列出、加、删元素 | 已有 `quiet board`；缺对齐、图层、复制、导出 SVG（中） |

底层操作 `load`（`quiet call load`，返回文档的路径、标题、rev 等，脚本里的 `lib.doc` 用它把文档载入脚本）和 `quiet open`（在应用里打开）是两回事。`load` 原来叫 `open`，2026-11-27 改名，免得和 `quiet open` 混淆。旧名 `open` 作为别名保留一个版本：`quiet call open`、`lib.call("open")` 仍可用，help 里不再列出，每次调用在 stderr 提示改用 `load`，下一个版本删除。`quiet open` 不经过 `Session::call`，脚本和 MCP 里也没有它：在应用里开窗口不该在沙箱脚本里发生。

## 分发

`quiet` 随应用一起发版，不单独发包。各平台怎么拿到（`help.rs` 总帮助末尾、skill 开头「`quiet` 在不在」一节和这里保持一致）：

- **macOS：** `.app` 里的 `Contents/MacOS/quiet`（`release.sh` 一起签名、公证）。应用「设置 › 连接 Agent › 安装命令行工具」软链到 `~/.local/bin/quiet`（另行实现），也可以自己 `ln -s`。Homebrew cask 有 `binary "#{appdir}/quiet.md.app/Contents/MacOS/quiet"`，用 cask 安装时直接进 PATH；仓库里的 cask 已经加上，tap 里那份要等第一个带 `quiet` 的版本（0.1.14 之后）发布时再加，否则旧版本安装会失败。
- **Windows：** `package-windows.ps1` 把 `quiet.exe` 和 `quiet_md.exe` 一起放进安装包（签名时一起签）。安装包有一个默认勾选的任务「Add the quiet command to PATH」：只改当前用户的 PATH（`HKCU\Environment`），已有就不重复加，取消勾选重装或卸载时去掉，`ChangesEnvironment=yes` 通知正在运行的程序。`self-test-windows.ps1` 检查装好后 `quiet.exe version` 能跑、PATH 里有安装目录，卸载后没有。
- **Linux：** tar 包里是 `bin/quiet`。AppImage 也带 `usr/bin/quiet`：以名为 `quiet` 的软链接调用（`ln -s …/quiet-md-*.AppImage ~/.local/bin/quiet`；AppImage 运行时把调用名放进 `ARGV0`），或 `quiet-md-*.AppImage --cli ARGS`，由 `AppRun` 转给 `quiet`；`package-appimage.sh` 打包后两种都验证。

## 数据表

CLI 能做应用里数据表的大部分事：读视图、按条件查、改字段和视图、管理一个文件里的几张表。全部走引擎同一套编辑（`ops::Doc`：`add_field`、`change_field`、`add_view`、`paste`、`remove_table`…）和查询（`query::run`、`query::stat`、`csv::export`），所以结果和应用一致：改类型按 `field::convert` 转换，双向关联的镜像字段跟着变，删表时指向它的关联变成文字。一次命令或一个脚本里的改动最后一次事务写入；`--dry-run` 列出要改的内容（包括要删的行和它们的标题）；运行中文件被改了报 `conflict`，什么都不写。

| 命令 | `Session::call` op | 脚本 |
| --- | --- | --- |
| `table DOC [--view V] [--where 'F OP V']… [--any] [--sort F[:desc]]… [--group F]… [--fields A,B] [--stats [S,…]] [--as json\|csv\|md] [--limit/--offset]` | `table` | `doc.table({view, where, any, sort, group, fields, stats, as})` |
| `table add` / `update ROW` / `update --where … --set …` / `delete ROW…\|--where …` | `table_add` / `table_update` / `table_delete` | `addRows`、`updateRow(s)`、`updateWhere`、`deleteRows`、`deleteWhere` |
| `table import DOC (--csv FILE\|- \| --rows JSON) [--create-fields]` | `table_import` | `doc.importRows(rows或CSV, {createFields})` |
| `table field add\|change\|rename\|delete` | `table_field_add` / `table_field_change` / `table_field_delete` | `addField`、`changeField`、`deleteFields` |
| `table view DOC`、`table view add\|set\|rename\|delete` | `table_views`、`table_view_add` / `_set` / `_rename` / `_delete` | `views`、`addView`、`setView`、`renameView`、`deleteView` |
| `table new\|rename\|remove DOC NAME` | `table_new` / `table_rename` / `table_remove` | `addTable`、`renameTable`、`removeTable` |

设计取舍：

- **名字跟着飞书和界面走。** 字段类型用 `FieldType::key()`（`single_select`），也接受界面上的中文名（`单选`）；视图类型 `grid|kanban|calendar|gantt|form`（也接受 表格/看板…）；筛选运算符 `= != ~ !~ > >= < <= empty notempty before after`，也接受 等于、包含、为空、早于…；相对日期 `today`、`this_week`、`past_7`…（`DateRef`），也接受 今天、本周、过去 7 天内。
- **条件先校验再用。** 应用里没填完的条件会被忽略；CLI 不这样：字段不存在报 `not_found`，运算符不适用于该字段类型报 `bad_args` 并列出可用的（`query::ops_for`），日期、数字、选项名读不出来也报错。日期字段上的 `<`/`>` 当作 早于/晚于。
- **临时条件叠在视图上。** `--where` 和视图自己的筛选同时成立；视图是「或」时先按视图筛一遍，再按给的条件筛（输出 `where` 写成 `(A or B) and C`）。`--sort`、`--group` 排在视图自己的前面。不带 `--view` 时是整张表，按表里的顺序。分组折叠是界面状态，CLI 读出全部行。
- **值是文字。** 读出的就是应用显示的文字（`field::display_in`）；写入按粘贴解析，单选缺的选项自动加上。关联字段写对方表主字段的文字或 `r12`，找不到报 `not_found`，重名报 `ambiguous`。
- **引擎会悄悄调整的，CLI 改成报错。** 字段、视图、表重名（引擎会改成「名字 2」），主字段改成不能当标题的类型（引擎会保持原类型），删主字段、删最后一个视图、删最后一张表：都报 `bad_args`/`exists` 并说明原因。被别的表关联的表要 `--force` 才删。
- **改字段类型报告丢失。** 输出转换成功和没转过来的格数，并列出前 20 个没转过来的行和原值；删选项报告受影响的格数；删字段报告哪些公式和查找引用会变成 `#引用失效`。
- **公式保存前检查。** `formula::check` 的位置换成字符序号：`bad_formula`（退出码 2），带出错的片段和字段列表。
- **`--stats`** 不指定时按类型给一组（数字 sum/average/min/max，日期 earliest/latest，勾选 checked/unchecked，其他 filled/unique）；带 `--view` 且视图给某列设了统计时用视图的。值是应用页脚显示的文字（`query::stat`）。
- **导入按表头对字段名。** 没有的列默认报错并列出；`--create-fields` 时按 `field::infer_type` 新建。`_id` 列忽略。

不做的：列宽、行高、冻结列、填色规则、分组折叠、看板封面、字段描述以外的说明性设置。这些是看的人的偏好，Agent 不需要，改了也看不出对错。`view set` 也不改视图类型（删了重建即可）。

## 第一版的不足和对应方案

| 问题 | 影响 | 方案 | 本轮 |
| --- | --- | --- | --- |
| 块 id 不跨调用 | 两次调用之间文档被改了，位置 ref 就失效，只能靠 rev 拒绝写入、重新读 | **自校验 ref**：ref 带上内容指纹，如 `#3~k7f2`。位置对得上、指纹也对得上才用。位置对不上时，按指纹在全文找唯一的块（块被挪了也能找到）。找不到再报 `stale_ref`。不改存储格式。 | ✅ |
| 应用开着同一篇 `.xml` 文档 | CLI 写完后，应用编辑器里还是旧内容，下次保存时弹「另一个实例修改」 | 应用把「编辑器没有未保存改动时自动载入磁盘新版本」从 Markdown 扩到所有文档。编辑器有未保存改动时，保留现有冲突提示。 | ✅ |
| CLI 打开文档库有副作用 | `Store::open_at` 会恢复上次异常退出留下的副本，这本该是应用的事 | 加 `Store::open_for_tools`，不做恢复 | ✅ |
| 数据表只读 | Agent 不能往表里加行、改单元格 | 行级操作：`table add/update/delete`，行有持久 id（`r12`）。值按字段类型解析（复用粘贴的解析）。按 sqlite 事务写入，应用按文件戳刷新。 | ✅ |
| 画板只读 | Agent 不能画图 | 画板读成元素列表。写入接受 Excalidraw 骨架（`convertToExcalidrawElements`，最少只要 type/x/y/label），追加到画板，或整体替换。 | ✅ |
| 改错了没法撤回 | Agent 只能手工改回去 | `history`：列出某篇文档的版本，看某个版本和当前的 diff，`restore` 恢复到某个版本（恢复本身也存一次快照）。回收站：`trash` 列出，`untrash` 恢复。 | ✅ |
| 不能导入文件 | 外部的 md/csv/excalidraw 进不了库 | `import FILE… [--in FOLDER]`：md（解析相对路径图片）、DocxXML、csv（转成数据表）、excalidraw | ✅ |
| 搜索太原始 | 只能整串匹配，结果没有排序 | 多个词同时出现才算命中（AND），标题命中和整串命中排在前面，命中多的排在前面 | ✅ |
| 一次读整篇大文档 | 输出可能几万字，浪费上下文 | `read` 默认最多 40k 字符，超出时提示按章节读。`--max-chars 0` 不限。 | ✅ |
| 没有逐命令帮助 | Agent 要读整份指南 | `quiet help CMD` | ✅ |
| 没有 shell 的 Agent 用不了 | Claude Desktop 之类只能用 MCP | `quiet mcp`：stdio MCP 服务，只有两个工具：`quiet`（命令行）和 `quiet_run`（脚本），schema 很小 | ✅ |
| 没装到 PATH | 用户拿不到 `quiet` | 见「分发」 | ✅（打包脚本） |
| 不知道 Agent 用得好不好 | 设计没验证 | 用模拟文档库和一组真实任务，让不同档位的模型只看 `quiet guide`（当时叫 `quiet skill`）去完成，记录成功率、调用次数和卡住的地方，再据此改 | ✅ |
| Windows 没编过 | QuickJS 是 C 代码 | 在 sida-windows 上编译并跑测试 | ✅ |
| CLI 保存不留历史版本 | 原来用 `Store::load` 读文档，没有登记编辑基线，`Store` 不存快照，改错了也无法 `restore` | 改用 `load_for_edit`，和编辑器一样登记基线。`restore` 前强制存一次快照，所以恢复本身也能撤销。 | ✅ |
| 写不出文档之间的链接（评估发现） | `[标题](路径.xml)` 会变成普通网页链接，读出时提及也只剩纯文本，复制章节会丢链接 | 写入时，`[[路径或标题]]` 和指向库内文件的相对链接都转成 @提及；找不到目标就报 `not_found`。按 Markdown 读出时，提及写回 `[[路径]]`，读出的内容可以原样写回。 | ✅ |
| 没法写到某节末尾（评估发现） | 只能写到全文末尾或某块之后，要先查最后一块的 ref | `--end-of 标题`（脚本里是 `{endOf}`） | ✅ |
| 按章节读会带上标题（评估发现） | 拆分文档时，标题既是文档名又出现在正文里 | `--no-heading`（脚本里是 `{heading: false}`），指南里的拆分示例也改用它 | ✅ |
| 看不出每个版本改了什么（评估发现） | `restore` 只能凭时间猜 | `history` 每个版本附「那次保存改了几行」（`then +3 -1`） | ✅ |
| 指南误导（评估发现） | 指南说新建的文档要到脚本结束才生效，三个模型都以为同一脚本里不能往新画板上画，多跑了一次 | 改写说明，补上画板示例；示例优先用脚本文件，不用 heredoc | ✅ |

## 用量统计

用来回答：每天多少人在用 CLI、用了多少次、用哪些命令。数据进 stats.quiet.md 看板的 CLI 版块。

**记什么：**
- 每台电脑一个随机 id，加 quiet 版本和系统。
- 每天的调用次数、失败次数、各命令名各几次。
- 不记文档、标题、路径、参数，不存 IP。

**怎么发：**
- 每次运行只在本地计数（`<数据目录>/quiet-md/cli-usage.json`）。
- 已经结束的日子，至多每小时一次，由后台的 `quiet __send-usage` 用系统 `curl` 发到 `POST telemetry.quiet.md/v1/cli`，不拖慢命令本身。
- 服务端是 `services/stats` Worker：存进 `cli_reports`，每天 00:30 UTC 汇总成 `cli.*` 指标。

**默认开启**（用户 2026-11-22 定的）：
- 第一次运行时在 stderr 说明一次。
- `quiet telemetry off` 关闭并忘掉 id，`quiet telemetry status` 查看本地记了什么。
- 以下情况完全不计数：设置了 `DO_NOT_TRACK` 或 `QUIET_NO_TELEMETRY`；在 CI 里；应用「设置 › 通用 › 网络与隐私」里关了「匿名使用统计」（没动过这个开关时跟随「自动检查更新」）。
- 应用设置里这个开关旁写明发什么、不发什么，用户在应用里就能看到并关掉（CLI 多数由 Agent 调用，首次提示用户本人看不到）。
- README 的隐私说明相应写明了这一点。

**看板：**
- 每日用户（含新用户）、每日调用（成功和失败堆叠）、近 7 天常用命令、昨日版本与系统分布。
- 平台筛选按系统：macOS 不分芯片。

## 评估

**评估库：** 指南库副本，加上 3 篇会议纪要、项目计划、方案、周报和一张书单数据表。
**任务：** 7 个：
1. 搜索
2. 往某一节的列表里加一项
3. 跨文档汇总待办
4. 改数据表
5. 按标题画流程图
6. 删除一节再撤销
7. 按章节拆分文档并在原文列出

**方式：** 每个模型只拿到 `quiet skill` 和任务描述，各用一份独立的库副本。完成后由脚本核对库里的结果。

| 模型 | 核对项通过 | quiet 调用次数 | 未通过 |
| --- | --- | --- | --- |
| Haiku | 12/13 | 21 | 提及链接（当时做不到） |
| Sonnet | 11/13 | 约 22 | 提及链接；拆分文档重复了标题 |
| Opus | 12/13 | 约 45（任务 7 花了约 25 次尝试写链接） | 提及链接 |
| Haiku，第二轮（改进后） | **13/13** | 37 | 无（用 `[[…]]` 写出了提及，拆分文档用了 `--no-heading`） |

第二轮又发现三处问题，已修复：
- **指纹相同：** 只含提及的块算出的指纹一样，因为指纹没算进行内对象。
- **参数写法：** `quiet delete 周报 风险` 这种分开写的块不认，现在等同于 `周报#风险`。
- **标题行：** 整篇读出时多一行 `# 标题`，容易被当成正文，现在只输出正文。

评估工具在 `tools/cli-eval/`，用法见那里的 README。

**结论：**
- 对模型能力要求不高，最小的模型也能完成全部任务。
- 失分和多余步骤都来自工具缺口和指南措辞，而不是模型理解力。上表「评估发现」的几行就是据此改的。
- 不同模型的用法差别：
  - 大模型更倾向用 `quiet run` 一次做完。
  - 小模型更多用单步命令。
  - 两种方式都能完成。

## 第二阶段（之后）

- **块 id 持久化。** 等应用里有「引用块」「块链接」这类需要稳定 id 的功能时一起做：DocxXML 加 `data-bid`，`FORMAT` 加一。届时自校验 ref 改成优先用 id。
- **和应用实时协作。** 应用在 `.quiet/` 下写一个在线标记，标出哪些文档开着、有没有未保存改动。CLI 写这些文档前先警告，或者等应用保存。更进一步，两边改的块不相交时做三方合并（基础版本取 `Store::loaded`），只有改到同一块才报冲突。
- **撤销栈对接。** 应用接收外部改动时，把它记成一个撤销步骤，人可以 ⌘Z 撤掉 Agent 的改动。
- **画板细粒度编辑。** 按元素 id 改文字、移动、删除。
- **PDF。** 等应用支持 PDF 后，`read` 输出文字，`outline` 输出书签。
- **设置里加「安装命令行工具」。** 在 `~/.local/bin` 或 `/usr/local/bin` 建软链接。
- **导入移进 core。** `quiet import` 和应用的 `transfer.rs` 现在各有一份 Markdown/CSV 导入代码。应用那份依赖 GPUI 侧的图片处理（`localize_pasted_images`），要先把它移进 core，两边再共用。
- **数据表的值类型。** 现在读出的值都是字符串。可以加 `--typed`，按字段类型给出数字、布尔和数组。
- **`read --as xml` 支持按块读。**
- **语义搜索。** 本地向量索引，`find --semantic`。

## 不做的事

- 不给脚本开文件、网络、进程权限。需要这些时，让 Agent 用自己的 shell，再把结果通过 `--md -` 传进来。
- 不另造一套查询语言。复杂查询用 JS 脚本写。
