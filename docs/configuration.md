# Pagewing 配置参考

配置路径：`~/.config/pagewing/config.toml`（Linux、macOS 一致），或 `$XDG_CONFIG_HOME/pagewing/config.toml`。`XDG_CONFIG_HOME` 必须为绝对路径；空值、相对路径按未设置处理。首次打开阅读器时生成配置；之后修改并重启生效。参考 [完整示例](../assets/config.example.toml)。

## 编辑器补全与校验

Pagewing 附带 [JSON Schema](../assets/config.schema.json)，覆盖所有配置字段和快捷键动作，提供说明、默认值、布局和适配模式的可选值，以及缩放范围校验。所有字段均可省略；拼错的字段或动作会得到提示。

每次打开阅读器时，程序会将内置 schema 放到配置文件旁的 `config.schema.json`，仅在内容变化时原子更新。schema 由程序维护，`config.toml` 仍保留用户的内容和注释。schema 写入失败不影响有效配置的加载。

新生成的配置已在顶部包含以下关联；已有配置需手动补上这一行，然后重新打开编辑器中的文件：

```toml
#:schema ./config.schema.json
```

这是 Taplo 支持的 [schema 注释](https://taplo.tamasfe.dev/configuration/directives.html)，路径相对于当前 TOML 文件，使用本地文件，无需网络。Neovim 启用 `taplo` LSP，VS Code 安装 **Even Better TOML** 后，即可获得补全、悬浮说明和校验。复制示例到其他目录时，也应复制 `config.schema.json`，或把注释改为 schema 的绝对路径；尚未启动新版本时可从仓库的 `assets/` 目录复制 schema。

若 Neovim 中 Taplo 已连接但没有补全，可在配置目录创建 `.taplo.toml`，内容为 `include = ["*.toml"]`，再重启 LSP。这让使用默认根目录规则的客户端将配置目录识别为 TOML 工作区，避免独立文件被排除。

schema 校验字段、值类型、枚举和数值范围；快捷键的具体按键名、保留键及动作之间的绑定冲突仍由阅读器检查。

## 外观与阅读视图

`[appearance]` 的 `dark`（深色界面）、`night`（页面反色）、`sidebar`（侧栏）、`thumbnails`（页面预览）均为布尔值，默认 `false`。这些值在每次启动时应用，阅读过程中仍可临时切换。

`text_embolden` 调节文字的显示加粗强度，范围 `0.0`–`1.0`，默认 `0.0` 保持原样。觉得 Times New Roman 等衬线字体偏细时，可先在已有的 `[appearance]` 表中加入 `text_embolden = 0.2`，仍偏细再试 `0.3` 或 `0.4`，修改后重启阅读器。加粗按字号等比例扩展字形轮廓，不改变字距、排版、文字选择和搜索位置；主页面、双页/连续阅读、缩略图、高倍缩放瓦片，以及白色/反转模式均生效。它只影响有填充的轮廓字体（如 TrueType、Type 1、CFF），不处理扫描图片中的文字、Type 3 字形、纯描边文字、文字裁剪轮廓或已转为普通矢量路径的文字，也不会加粗图片和图表线条。源 PDF 与 `--render` 导出保持原样。

```toml
[appearance]
text_embolden = 0.2
text_embolden_step = 0.05

[shortcuts]
text_embolden_increase = ["Alt+Equals"]
text_embolden_decrease = ["Alt+Minus"]
```

阅读时默认按 `Alt+=` 增加、`Alt+-` 减少文字加粗（macOS 的 Alt 为 Option）；每次变化量由 `appearance.text_embolden_step` 决定，范围 `0.001`–`1.0`，默认 `0.05`。支持数字计数，例如 `3` 后按增加键会增加三个步长。结果限制在 `0.0`–`1.0`，立即刷新页面、缩略图及高倍缩放瓦片，并在状态栏显示当前值。快捷键可像其他动作一样改绑或用 `[]` 停用；在搜索框、评论编辑等输入区域不会触发。调节只对当前阅读器会话生效，不改写配置文件，重启后恢复 `text_embolden` 配置的初始值。

`I` 在白色模式和反转模式间切换，分别使用 `[appearance.white]` 与 `[appearance.inverted]`；`D` 独立切换界面主题。两个配色表都支持 `foreground`（原 PDF 黑色像素的显示颜色）、`background`（原 PDF 白色像素/纸张的显示颜色）和可选的 `canvas`（纸张外围的底色）。颜色必须为 `"#RRGGBB"`，大小写均可。省略 `canvas` 时沿用界面主题底色。

白色模式默认黑字白纸，反转模式默认白字黑纸；只配置部分字段时，其余字段仍使用对应模式的默认值。下面将反转模式配置为 [Catppuccin Mocha](https://catppuccin.com/palette/) 的 Text、Base 和 Mantle，并在启动时启用：

```toml
[appearance]
dark = true
night = true

[appearance.white]
foreground = "#000000"
background = "#ffffff"

[appearance.inverted]
foreground = "#cdd6f4"
background = "#1e1e2e"
canvas = "#181825"
```

配色应用于主页面、双页/连续阅读、缩略图和高倍缩放瓦片，只改变显示，不修改 PDF 或导出内容。每个 RGB 通道在前景与背景之间线性映射，保留抗锯齿及灰阶过渡；彩色图片、图表也会参与映射（反转模式会改变其颜色）。修改后重启阅读器生效。

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
| `zoom` | Set an exact zoom percentage (`Cmd+Alt+Z`) |
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

未知字段、未知动作、无效颜色、无效按键、快捷键冲突和非法缩放会报告错误。程序显示配置路径并使用内置默认设置，保留原配置文件，修正后重启即可。

`config.toml` 只由用户编辑，阅读进度保存不会改写它。`library.json` 继续保存最近文档、阅读位置、书签及会话状态；其中旧的偏好字段只用于首次迁移，之后启动时以 TOML 配置为准。Linux 的阅读记录位于 `$XDG_CONFIG_HOME/pagewing/library.json`（默认 `~/.config/pagewing/library.json`），macOS 位于 `~/Library/Application Support/dev.pagewing.Pagewing/library.json`。
