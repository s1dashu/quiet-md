# 打字机模式：调研与做法

打字机模式（typewriter mode / typewriter scrolling）让正在写的那一行停在窗口里一个固定的高度，通常是中间，写作时视线不用跟着光标往下走。本文先列各家怎么做（2026-11-25 调研，每条注明出处），再写 Quiet.md 的做法和待定的分歧。实现以代码为准（`src/typewriter.rs`、`Editor::apply_reveal`），对不上时以代码为准。

## 各家做法

| | 光标行位置 | 什么时候滚 | 点击 | 滚轮手动滚后 | 首尾留白 | 动画 | 配套变淡 | 入口 / 快捷键 / 记忆 |
|---|---|---|---|---|---|---|---|---|
| 熊掌记 Bear 2 [1] | 中间 | 未写明 | 未写明 | 未写明 | 未写明 | 未写明 | 页面未提 | 编辑 › 打字机模式，⌃⌘T（仅 Mac） |
| iA Writer [2] | 中间 | 打字、上下移动 | 未写明 | 未写明 | 未写明 | 未写明 | **捆绑**：专注模式三选一（打字机 / 句子 / 段落），打字机不变淡 | 专注 › 开启专注模式，⌘D；建议选中编辑时关掉，否则屏幕会跳 |
| Ulysses [3] | 上 / 中 / 下 可选，或「可变」（点击、方向键不锁，开始打字才锁在当前高度） | 打字 | 可变模式下不滚 | 未写明 | 未写明 | 未写明 | 独立：高亮当前行 / 句 / 段，其余变淡；可加当前行底色 | 显示 › 专注写作 |
| Typora [4][5] | 中间 | 打字；默认点击改变光标也会居中 | **默认会滚**，偏好里可关成「只在打字时」 | 未写明 | 未写明 | 未写明 | 独立：专注模式 F8（当前行 / 块之外变淡） | 显示菜单，F9（三平台同键） |
| Obsidian 核心 | 无此功能 | | | | | | | |
| Obsidian 插件 Typewriter Mode [6] | 滑块 0–100%，默认 50% | 打字、删除、撤销重做、键盘移动光标；可关「方向键也居中」 | **不滚** | 不拉回，下次编辑或按键再居中 | 默认上下都留；可选「到达位置后才保持」（只留底部） | 无，立即跳 | 独立：段落 / 句子变淡、当前行高亮；**选中时暂停变淡**、**滚动时暂停变淡** | 全局设置，可按文件用 frontmatter 关；表格单元格内不做滚动 |
| Obsidian 插件 Typewriter Scroll [7] | 可调偏移 | 方向键、打字 | 鼠标选择不滚 | 未写明 | 文档开头也能居中 | 未写明 | 「Zen 模式」：当前行外变淡 | 设置或命令 |
| Scrivener 3 [8][9] | Mac 可选 1/4、1/3、中间；Windows 不能调 [10] | 打字 | 点击或键盘跳多行后，**把新位置当作新的固定行**，不跳；⌘J 回到设定行；可勾「总是跳回设定行」 | 未写明 | 结尾加空行（所以页面视图里不可用） | 未写明 | — | 显示 › 文本编辑 › 打字机滚动，⌃⌘T / Ctrl+Win+T；每个分栏、全屏写作各自独立 |
| FocusWriter [11] | 中间 | 默认光标碰到上下边才居中；可选「总是居中」 | 按住鼠标时不滚 | 不拉回，下次移动光标再居中 | 只在结尾多一屏 | 无，立即跳 | 独立：当前行 / 前三行 / 当前段之外 50% 透明 | 偏好设置 |
| Drafts [12] | 中间 | 未写明 | | | | | 独立：当前行底色 | 编辑器设置里的开关 |
| VS Code 插件 [13][14][15] | 上 / 中 / 下 或按行数偏移 | 换行时 | 忽略鼠标 | | | 一款有「平滑」变体（偏离才回） | | 状态栏开关等 |
| **Notion** | **没有这个功能**：官方快捷键页没有打字机、专注或居中相关条目 [16]，也没找到帮助文章。（证据是官方文档里没有，不是官方明说没有） | | | | | | | |
| Word / Google Docs | 都没有。Word 官方社区答复是没有，只能用分屏或空段落凑 [17]；Google Docs 只有第三方扩展 [18] | | | | | | | |

几项细节没有任何一家写明：长段落、代码块、图片里的表现，输入法组字时的表现（只有 Obsidian 插件写了表格）。粘贴、撤销是否触发，只有 Obsidian 插件和 VS Code 插件写了（会触发）。

## 共识与分歧

**多数这样做**

- 光标行默认停在**正中间**；能调的（Scrivener、Ulysses、Obsidian 插件）是预设档位或百分比滑块。
- **打字一定居中，方向键通常也居中，鼠标点击通常不滚**（Obsidian 两款插件、VS Code 插件、Scrivener、FocusWriter）。
- **滚轮手动滚开后不会被自动拉回**，下次按键或移动光标再回到中间（Obsidian 插件、FocusWriter）。
- **「其他段落变淡」是另一个独立开关**（Typora、Ulysses、Obsidian、FocusWriter）；只有 iA Writer 把打字机放进专注模式里三选一。
- 写明首尾处理的都会**留空白让最后一行能到中间**；结尾留白是共识（Scrivener、FocusWriter），开头要不要留有分歧。
- 记忆：全局记住（Typora、Obsidian 插件、Drafts）居多。

**有分歧**

1. 点击要不要居中：Typora 默认会；Scrivener 把点击处当新的固定行；多数不滚。
2. 什么时候开始锁定：一开就居中（Typora、iA Writer），还是等光标走到那条线或开始打字才锁（Ulysses「可变」、Scrivener 默认、FocusWriter 默认、Obsidian「到达后才保持」）。
3. 开头要不要留半屏空白，让第一行也能停在中间。
4. 位置能不能调。
5. 动画：有源码可查的（Obsidian 插件、FocusWriter）都是立即跳。

## Quiet.md 的做法（实验分支）

照上面最普遍、最不打扰的做法：

- **位置**：正中间（`TYPEWRITER_LINE = 0.5`），不提供调节。
- **触发**：凡是「键盘带光标走」的操作都居中：打字、回车、删除、方向键、⌘↑↓、粘贴、撤销重做、斜杠菜单插入；查找跳到的命中也停在中线。超过一屏的跳转（粘贴大段、撤销到远处、光标不在屏上）直接到位，不长距离滑动。实现上就是编辑器原有的「显示光标」（`Editor::changed` → `Reveal::Caret`），打字机模式只把「贴边就停」换成「对准中线」，所以覆盖面和原来的滚动意图一致。
- **鼠标点击不滚**：点击本来就不触发显示光标（`tools/harness/scroll_intent.py` 守着这一点）；点完接着打字，下一下键盘再平滑居中。鼠标拖选、拖块也不滚。
- **选中文字时不居中**：有选区（Shift+方向键、鼠标选、整块选中）时退回普通的贴边显示，避免 iA Writer 提醒的「选择时屏幕乱跳」，也不会在点工具栏「加粗」时把选区挪走。
- **输入法组字时不居中**：组字期间（有 marked text）照普通规则，提交后下一次再居中，候选窗不会跟着页面跳。
- **长块**：按光标所在的那一**行**对准中线，长段落、代码块、表格单元格里上下移动都按行走；比窗口还高的目标（例如整块选中一张很高的图）退回普通显示。
- **首尾**：结尾留出半屏空白（`below_height`，至少原来的 200px），最后一行也能到中间；开头**不**加空白，第一屏光标在自然位置，往下写到中线后才开始固定（等于 Scrivener / FocusWriter 的「到达后才锁」）。
- **滚轮**：手动滚开后不拉回；滚轮会打断正在进行的平滑滚动；下一次按键再回到中间。
- **窗口变化**：缩放窗口、收起 / 展开侧栏、进出专注模式时，光标行原本在中间就立即留在中间（不滑动）；滚轮滚开过、或点击把光标放在别处时不动。
- **动画**：跟「设置 › 外观 › 动画」：丰富 = 平滑滑过去（剩余距离按 e^(−t/60ms) 收拢，约 0.2 秒到位，每帧按最新光标位置重新瞄准，连续打字不会抖）；克制 = 立即到位。
- **阅读模式**：不起作用，也不加结尾空白。
- **入口**：文档「…」菜单最上面一组，「阅读模式」下面：「打字机模式」开关，悬停说明「正在写的一行停在窗口中间」。快捷键 **⌘⇧T**（Windows / Linux **Ctrl+Shift+T**），用快捷键切换时有提示。可在设置 › 快捷键里改。
- **记忆**：全局偏好（`preferences.json` 的 `typewriter`），所有文档、重启后都保持。
- **聚焦变淡**：见下面「段落聚焦」，是另一个独立开关。「专注模式」（`src/focus.rs`，隐藏顶栏、侧栏、红绿灯）只管窗口外观，不变淡，三个开关互不依赖，可以叠加。

测试：`tools/harness/typewriter.py`（打字、回车、方向键、开头、结尾、滚轮后再打字、点击、选中、关闭时、快捷键开关和记忆、克制动画、约 100 万字长文档打字耗时）。

## 已定（用户 2026-11-25：「照推荐」）

1. **点击不居中**：点击只放光标，页面不动，下一次按键再居中。
2. **位置固定在正中间**，不提供调节。
3. **开头不留空白，只在结尾留半屏**：第一屏光标在自然位置，最后一行能到中间。
4. **「其他段落变淡」这次不做**；以后做就做成独立开关，不并进打字机模式。
5. **快捷键 ⌘⇧T / Ctrl+Shift+T**，不跟熊掌记、Scrivener 的 ⌃⌘T。
6. **全局记住**（`preferences.json` 的 `typewriter`），不按文档记。

以上和实现一致。仍在实验分支，用户试过预览、说进 main 再推。

## 段落聚焦（2026-11-26 加）

用户要「其他段落变淡」（iA Writer 的 Focus、Typora 的专注模式、Ulysses 的高亮都有）。照 11-25 的决定做成**独立开关**，不并进打字机模式，也不并进专注模式。

- **入口**：文档「…」菜单，紧跟「打字机模式」：「段落聚焦」开关，悬停说明「正在写的一段保持清晰，其余变淡」。**默认关**（各家都是用户主动打开；一打开整页变淡，对没准备的人太突兀）。全局记住（`preferences.json` 的 `paragraph_focus`），快捷键 ⌘⇧F（Windows / Linux Ctrl+Shift+F），切换时有提示，可在设置 › 快捷键里改。
- **什么算「当前」**：光标所在的那一行列表项：一个段落、标题、一个列表项，或整个容器（引用、高亮块、分栏、表格、代码块算一个）。页面标题也是一项。选区跨多段时，从起点到终点的每一项都保持清晰；整块选中同理。
- **怎么变淡**：整项降透明度到 0.3（`FOCUS_DIM`），文字、图片、表格边框、代码块一起淡，不只改文字颜色。理由：只改文字色时，表格边框、图片、代码块底色、引用竖线都还是原样，页面会花；统一透明度和 Typora、Obsidian 插件的做法一致，深浅色自动都对，不用给每种主题配色。
- **过渡**：换段落、打开和关闭开关都 150ms 缓出（`FOCUS_FADE`），跟「动画」设置：丰富渐变，克制立即切换。
- **不影响**：阅读模式不变淡；封面不变淡；块手柄、悬浮工具栏、菜单不受影响。
- **和打字机模式**：互不依赖，常一起用；只开段落聚焦时页面照常滚动。

测试：`tools/harness/typewriter.py` 的 `focus`（浅、深色下光标段比其他段墨色浓、跨段选区三段都清晰、关掉时不变淡）、`focus_key`（⌘⇧F 开关和记忆），以及 `find`、`resize`、`paste`、`ime`。

## 对照：他们有、我们缺（2026-11-26 补齐）

| 体验 | iA Writer | Typora | Ulysses | Quiet.md |
|---|---|---|---|---|
| 光标行居中 | 有 | 有 | 有（上 / 中 / 下 / 可变） | 有，固定中间（11-25 定） |
| 其他段落变淡 | 有（句子 / 段落，和打字机三选一）| 有（专注模式 F8）| 有（行 / 句 / 段）| **补上**：段落聚焦，独立开关，⌘⇧F |
| 变淡开关的快捷键 | ⌘D | F8 | 菜单 | **补上** ⌘⇧F / Ctrl+Shift+F |
| 进出模式的过渡 | 未写明 | 未写明 | 未写明 | **补上**：打开打字机时当前行滑到中间；段落聚焦开关、换段落都 150ms 渐变（克制时立即） |
| 查找跳转后居中 | 未写明 | 未写明 | 未写明 | **补上**：打字机模式下命中停在中线 |
| 大段粘贴 / 远处撤销 | 未写明 | 未写明 | 未写明 | **补上**：超过一屏直接到位 |
| 窗口缩放、侧栏收起后保持 | 未写明 | 未写明 | 未写明 | **补上**：原本居中就保持居中 |
| 输入法组字不跳 | 未写明 | 未写明 | 未写明 | 有：组字时不动，提交后居中 |
| 当前行底色（line marker）| 无 | 无 | 有 | 不做：和段落聚焦重复，页面多一层色块 |
| 句子级变淡 | 有 | 无 | 有 | 不做：中文句子边界（。！？…和引号）不稳，段落已够用；以后需要再加档位 |
| 光标位置可调 | 无 | 无 | 有 | 不做（11-25 定） |
| 点击也居中 | 未写明 | 默认会 | 可变模式下不 | 不做（11-25 定：点击不跳） |
| 开头也能居中（上方留白）| 未写明 | 未写明 | 未写明 | 不做（11-25 定：只在结尾留，11-26 总控再确认） |
| 和全屏 / 专注界面的关系 | 专注模式本身不藏界面 | 同左 | 打字机和「隐藏界面」各自独立 | 三个独立开关：打字机、段落聚焦、专注模式（藏界面），可任意叠加 |

## 出处

1. Bear FAQ: Typewriter mode — https://bear.app/faq/typewriter-mode/
2. iA Writer: Focus Mode — https://ia.net/writer/support/editor/focus-mode
3. Ulysses: Editor customization guide — https://help.ulysses.app/en_US/dive-into-editing/editor-customization-guide
4. Typora: Focus and Typewriter Mode — https://support.typora.io/Focus-and-Typewriter-Mode/
5. Typora: Shortcut Keys — https://support.typora.io/Shortcut-Keys/
6. Obsidian Typewriter Mode（davisriedel，读了源码）— https://github.com/davisriedel/obsidian-typewriter-mode
7. Obsidian Typewriter Scroll（deathau，更新日志）— https://github.com/deathau/cm-typewriter-scroll-obsidian
8. Scrivener 3 Mac 手册 §16.3 — https://www.literatureandlatte.com/docs/Scrivener_Manual-Mac.pdf
9. Scrivener 3 Windows 手册 — https://www.literatureandlatte.com/docs/Scrivener_Manual-Win.pdf
10. Scrivener 论坛：Windows 没有打字机行位置设置 — https://forum.literatureandlatte.com/t/typewriter-scroll-line-feature-for-windows/142067
11. FocusWriter 源码 `src/document.cpp`、`src/preferences_dialog.cpp` — https://github.com/gottcode/focuswriter
12. Drafts: Editor Settings — https://docs.getdrafts.com/docs/editor/editorsettings.html
13. VS Code 插件 Center Editor Window — https://marketplace.visualstudio.com/items?itemName=kaiwood.center-editor-window
14. VS Code 插件 Typewriter Auto-scroll — https://marketplace.visualstudio.com/items?itemName=MikeyLau.typewriter-auto-scroll
15. VS Code 插件 Typewriter Scroll — https://marketplace.visualstudio.com/items?itemName=LamborGhinious.typewriter-scroll
16. Notion 快捷键 — https://www.notion.com/help/keyboard-shortcuts
17. Microsoft 社区：Word 没有打字机模式 — https://learn.microsoft.com/en-us/answers/questions/4839236/typewriter-mode-(not-scrolling-while-writing)?forum=msoffice-all
18. Lamplighter（Google Docs 扩展）— https://chromewebstore.google.com/detail/lamplighter-night-mode-wr/aeolpddnncgcjfnjknblkibkpoogkkai
