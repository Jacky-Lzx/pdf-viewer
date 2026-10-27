# Pagewing 配置参考

配置路径：`~/.config/pagewing/config.toml`（Linux、macOS 一致），或 `$XDG_CONFIG_HOME/pagewing/config.toml`。`XDG_CONFIG_HOME` 必须为绝对路径；空值、相对路径按未设置处理。首次打开阅读器时生成配置；之后修改并重启生效。参考 [完整示例](../assets/config.example.toml)。

## 外观与阅读视图

`[appearance]` 的 `dark`（深色界面）、`night`（页面反色）、`sidebar`（侧栏）、`thumbnails`（页面预览）均为布尔值，默认 `false`。这些值在每次启动时应用，阅读过程中仍可临时切换。

`[display]`：

| 字段 | 可用值 | 默认值 |
| --- | --- | --- |
| `layout` | `"single"` 单页、`"continuous"` 连续滚动、`"facing"` 双页 | `"single"` |
| `fit` | `"page"` 整页、`"width"` 宽度、`"height"` 高度，或 `0.05`–`8.0` 的缩放倍数 | `"page"` |
| `remember_documents` | `true` 恢复每个文档的位置与视图；`false` 每次从第 1 页按默认视图打开 | `true` |

首次生成配置时继承旧设置；手工配置中省略的字段使用内置默认值。书签和命名位置不受 `remember_documents` 影响，`--page` 可指定起始页。

## 快捷键

在 `[shortcuts]` 下，以动作名为键、按键字符串数组为值。某动作出现时会替换它的全部默认绑定；空数组 `[]` 停用该动作的快捷键，省略动作则保留默认绑定。

```toml
[shortcuts]
next_page = ["X", "Shift+J"]
previous_page = ["Shift+X"]
night = []
```

修饰键为 `Cmd`、`Ctrl`、`Alt`、`Shift`，使用 `+` 连接。`Cmd` 在 macOS 表示 Command，在 Linux 表示 Ctrl；`Ctrl` 在 macOS 表示物理 Control。按键名例如 `A`、`ArrowDown`、`PageDown`、`Home`、`End`、`Space`、`Tab`、`Comma`、`Plus`、`Minus`、`Equals`、`Slash`、`Questionmark`、`Colon`、`OpenBracket`、`CloseBracket`、`F1`–`F12`。修饰键和具名按键按上述大小写填写；单个字母不区分大小写。

同一个按键不能分配给多个动作（包括未覆盖动作的默认绑定）。若要改用其他动作已有的按键，先覆盖或停用原动作。Escape、Enter、Backspace、文本编辑组合键、数字计数和 Vim 序列前缀保留。原 Settings 的 `Cmd+Comma` 已释放，可以分配给其他动作。阅读快捷键在输入框中不会触发阅读操作。

| 动作名 | 功能 |
| --- | --- |
| `open` | Open PDF |
| `close` | Close PDF |
| `reopen` | Reopen closed PDF |
| `recent` | Find recent document |
| `reload` | Reload PDF |
| `quit` | Quit |
| `find` | Search document |
| `comments` | Toggle comments |
| `add_comment` | Add highlight comment |
| `down` | Scroll down |
| `up` | Scroll up |
| `left` | Scroll left |
| `right` | Scroll right |
| `next_page` | Next page |
| `previous_page` | Previous page |
| `first_page` | First page |
| `last_page` | Last page / counted page |
| `screen_down` | Screen down |
| `screen_up` | Screen up |
| `half_down` | Half screen down |
| `half_up` | Half screen up |
| `scroll_down` | Scroll down without turning pages |
| `scroll_up` | Scroll up without turning pages |
| `history_back` | Jump history back |
| `history_forward` | Jump history forward |
| `next_bookmark` | Next bookmark |
| `previous_bookmark` | Previous bookmark |
| `zoom_in` | Zoom in |
| `zoom_out` | Zoom out |
| `actual_size` | Actual size (100%) |
| `fit_page` | Fit page |
| `fit_width` | Fit width |
| `fit_height` | Fit height |
| `rotate` | Rotate clockwise |
| `rotate_back` | Rotate counterclockwise |
| `continuous` | Toggle continuous scrolling |
| `contents` | Find chapter |
| `properties` | Document information |
| `links` | Follow visible link |
| `copy_link` | Copy visible link address |
| `next_match` | Next search match |
| `previous_match` | Previous search match |
| `bookmark` | Toggle bookmark |
| `sidebar` | Toggle sidebar |
| `thumbnails` | Toggle page previews |
| `dark` | Toggle dark interface |
| `night` | Invert page colors |
| `selection` | Toggle text selection |
| `copy` | Copy selection / page text |
| `goto` | Go to page dialog |
| `help` | Keyboard shortcuts |
| `fullscreen` | Toggle fullscreen |

## 错误与阅读记录

未知字段、未知动作、无效按键、快捷键冲突和非法缩放会报告错误。程序显示配置路径并使用内置默认设置，保留原配置文件，修正后重启即可。

`config.toml` 只由用户编辑，阅读进度保存不会改写它。`library.json` 继续保存最近文档、阅读位置、书签及会话状态；其中旧的偏好字段只用于首次迁移，之后启动时以 TOML 配置为准。Linux 的阅读记录位于 `$XDG_CONFIG_HOME/pagewing/library.json`（默认 `~/.config/pagewing/library.json`），macOS 位于 `~/Library/Application Support/dev.pagewing.Pagewing/library.json`。
