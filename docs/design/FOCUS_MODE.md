# 专注模式：参考产品和结论

状态：2026-11-25 用户试过后合进 main。实现在 `src/focus.rs`，和本文对不上时以代码为准。

## 参考产品

| 产品 | 叫法 | 入口 | macOS | Windows | 退出 | 藏什么 | 记住吗 |
|---|---|---|---|---|---|---|---|
| Ulysses | Hide Interface（原 Editor Focus） | 显示菜单 | ⌘. | 无 Windows 版 | 再按 ⌘. | 资料库、文稿列表、工具栏，**连红绿灯都藏**；不进全屏 | 没写 |
| Craft | Focus Mode | 左上角图标 | ⌘. | 无 | 图标、**Esc**、再按 ⌘. | 所有侧栏和顶栏 | 没写 |
| Scrivener | Composition Mode | 工具栏、显示菜单 | ⌘⌥F | F11 | **Esc**、控制条上的退出按钮 | 除正文外全部，占满屏幕 | 没写 |
| iA Writer | Focus Mode | Focus 菜单、标题栏按钮 | ⌘D | Ctrl+Shift+D | 再按一次 | 不藏界面，只把其他句子调暗；全屏另算（⌃⌘F） | 没写 |
| Typora | Focus Mode | 显示菜单 | F8 | F8 | 再按一次 | 不藏界面，只把其他段落调暗 | 没写 |
| Bear 2 | Show Editor Only | 显示菜单 | ⌃1 | 无 | 显示菜单换回布局 | 侧栏和笔记列表 | 没写 |
| Obsidian | 无内置（Zen、Hider 插件） | 命令、插件 | 无 | 无 | 插件留的恢复按钮 | 侧栏 | — |
| Notion | 无，只有「全宽」 | ••• 菜单 | 无 | 无 | — | — | 按页记住全宽 |
| 飞书文档 | 无专注模式，只有页宽设置 | 右上角 › 页宽设置 | 无 | 无 | — | — | 按文档、按人记住页宽 |

出处：
- Ulysses：https://help.ulysses.app/en_US/dive-into-editing/editor-customization-guide ，https://help.ulysses.app/en_US/general/keyboard-shortcuts-mac-ipad
- Craft：https://support.craft.do/en/introduction/shortcuts ，https://documents.craft.me/fegziZPWTrBfn0
- Scrivener：https://www.literatureandlatte.com/blog/distraction-free-writing-with-scrivener
- iA Writer：https://ia.net/writer/support/editor/focus-mode ，https://ia.net/writer/support/basics/shortcuts/keyboard-shortcuts-windows
- Typora：https://support.typora.io/Focus-and-Typewriter-Mode/ ，https://support.typora.io/Shortcut-Keys/
- Bear：https://bear.app/faq/hide-the-sidebar-and-note-list-on-mac-and-ipad/
- Obsidian：https://community.obsidian.md/plugins/zenmode
- Notion：https://www.notion.com/help/customize-and-style-your-content
- 飞书：https://www.feishu.cn/hc/zh-CN/articles/909135942944
- macOS 全屏时鼠标移到顶部才出现菜单栏和工具栏：https://support.apple.com/guide/mac-help/customize-toolbars-mchlb294260e/mac

几点观察：

1. **快捷键**：Mac 上「藏界面」最通行的是 ⌘.（Ulysses、Craft）。⌘D、F8 是「调暗其他文字」，不是一回事；全屏几乎都是 ⌃⌘F（Windows F11）。
2. **退出**：再按同一个快捷键人人都支持；藏得最彻底的两个（Craft、Scrivener）还支持 Esc。
3. **和系统全屏分开**：都不强制进系统全屏，两者可以叠加。只有 Ulysses 在窗口状态下也把红绿灯藏起来。
4. **顶部出现**：没有产品在文档里写「鼠标移到顶部出现工具栏」，最接近的是 macOS 全屏本身（顶部出现菜单栏和标题栏）和 Scrivener（底部出现控制条）。
5. **记不记住**：没有一家写明专注模式重开后还在；明确记住的只有页宽这类「这篇怎么看」的设置（Notion、飞书）。

## 结论（实现）

- **入口**：文档「…」菜单第一组，「阅读模式」（和打字机模式）后面、「显示引用列表」前面，叫「专注模式」，和上下几项一样是开关行，悬停说明「只留正文，鼠标移到顶部可退出」带快捷键。点开关，滑块先在菜单里滑到开（0.15 秒），然后菜单随整个界面一起收起：菜单挂在顶栏的「…」上，专注时「…」不在了，留着菜单会悬空。开关直接读当前是否在专注模式，不另存；专注时没有菜单可点，用任何方式退出后再打开菜单，开关是关的。只在文档页出现，数据表和画板文件没有。
- **快捷键**：macOS ⌘.，Windows Ctrl+Shift+Enter，进入和退出同一个键，快捷键设置页「文档」组里叫「进入 / 退出专注模式」，可以改。Windows 不用 Ctrl+.（微软拼音的中英文标点切换）、Ctrl+Shift+F（微软拼音的简繁切换）、F11（已经是全屏）。
- **专注中**：侧栏收起，文档大纲收起，顶栏的按钮、面包屑和保存状态都不显示，只留一条和顶栏一样高的空白（照样能拖动窗口、双击缩放），正文不跳动。macOS 红绿灯藏起来（藏起来时也不能点）；Windows 右上角的窗口按钮同理。
- **顶部出现**：鼠标进到这条空白里，红绿灯和右上角的「✕ 退出」按钮出现（ghost 按钮，和平时顶栏右侧那排按钮同一行、同样的右边距，红绿灯位置和平时一样）；离开 0.4 秒后消失（路过顶部去菜单栏、手稍微过头都不会闪）。「动画」设成丰富时淡入、淡出都是 0.15 秒，克制时直接出现和消失。系统全屏时红绿灯归系统管，不动。
- **退出**：顶部按钮、同一个快捷键、Esc。编辑器里有别的东西要 Esc 先处理（斜杠菜单、@ 提及、查找条、块选择、表格选区、裁图、版本历史）时先处理那些；没有了，第一下 Esc 就退出专注模式，而不是平时的「选中当前块」。⌘] 展开侧栏也会退出专注模式。离开文档页（回主页、进设置）自动退出。
- **恢复**：侧栏回到进入前的状态（开着就开，关着就关，窗口太窄自动收起的也照旧）。页宽、字号、阅读模式这些文档自己的设置专注时照旧生效，进出都不改。
- **不记住**：跟参考产品一致，也跟「窗口保持在最前」一样，关掉窗口或重开应用后是正常界面。专注是一时的状态，窗口一直空着容易让人以为东西丢了。
- **不弹提示**：进入时什么都不弹。鼠标移到顶部，红绿灯和「退出」一起出现，自然知道怎么出去。
