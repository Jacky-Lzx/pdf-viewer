<img src="assets/icon.svg" width="64" height="64" alt="Folio">

# Folio

用 Rust 编写的原生 PDF 阅读器，支持 Linux（Wayland / X11）和 macOS。使用 eframe / egui 构建界面，hayro 在后台进行纯 Rust PDF 渲染，无需安装 PDFium 或 Poppler。

<img src="assets/screenshot.png" width="1000" alt="Folio：文档目录、中文文件名和键盘链接提示">

## 运行

需要 Rust **1.92 或更新版本**。

```sh
cargo run --release -- assets/folio-guide.pdf
# 打开空白阅读器，然后选择或拖入自己的文档
cargo run --release
# 安装命令行程序
cargo install --path . --locked
folio /path/to/book.pdf
```

Linux 需要正常工作的桌面会话、OpenGL、X11 或 Wayland 及 libxkbcommon。Ubuntu / Debian 可安装：

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev libgl1-mesa-dev libx11-dev libxi-dev libxrandr-dev
```

NixOS / Nix 用户可以在 `nix-shell` 中构建和运行。Linux 文件选择器使用 XDG Desktop Portal，需要桌面环境对应的 portal 服务。macOS 使用系统文件选择器；运行 `bash scripts/bundle-macos.sh` 可把 release 构建打包为 `target/Folio.app`。

中文文件名、目录和搜索框会自动使用系统中文字体。Linux 可安装 `fonts-noto-cjk`；macOS 使用系统字体。也可以设置 `FOLIO_FONT=/path/to/font.otf` 指定回退字体，字体在后台加载。

## 键盘操作

快捷键只在阅读模式生效，输入搜索词或页码时不会触发页面导航。按 `?` 查看内置帮助。

| 按键 | 操作 |
| --- | --- |
| `j` / `k` / `h` / `l` | 向下 / 上 / 左 / 右滚动；上下到达边缘后翻页 |
| `J` / `K`，`]` / `[` | 下一页 / 上一页 |
| `gg` / `G` | 首页 / 末页 |
| `42G`，`42 Enter`，`42gg` | 跳到第 42 页 |
| `5j`，`2l`，`3J`，`3]`，`2 Space` | 带数字前缀的四向滚动、翻页和整屏滚动；数字与按键之间无需空格 |
| `Ctrl+d` / `Ctrl+u` | 下移 / 上移半屏 |
| `Space` / `Shift+Space` | 下移 / 上移一屏 |
| `Alt+Left` / `Alt+Right` | 返回 / 前进阅读位置历史 |
| `+` / `-` | 放大 / 缩小 |
| `p` / `w` | 适合整页 / 适合宽度 |
| `r` / `Shift+r` | 顺时针 / 逆时针旋转 |
| `/`，`Ctrl/Cmd+f` | 全文搜索 |
| `n` / `N` | 下一个 / 上一个搜索匹配 |
| `f`，然后输入标签 | 跟随当前可见的 PDF 链接；`Escape` 取消 |
| `b` | 添加 / 删除当前页书签 |
| `ma`，`'a` 或 `` `a `` | 保存 / 返回命名位置，字母可用 `a`–`z`；同时恢复缩放、旋转和页内滚动 |
| `Tab` | 打开 / 关闭侧栏 |
| `d` / `i` | 切换深色界面 / 反转页面颜色 |
| `y` | 复制当前页文本 |
| `v`，拖选，`y` 或 `Ctrl/Cmd+C` | 选择并复制部分文本；`v` 或 `Escape` 退出选择模式 |
| `o`，`Ctrl/Cmd+o` | 打开文档 |
| `Ctrl/Cmd+r` | 重新加载当前 PDF，保留阅读位置和标记 |
| `:` | 页码跳转对话框 |
| `F11` | 全屏 |
| `Escape` | 关闭对话框或搜索，取消数字前缀 |

鼠标滚轮或触控板滚动，拖动平移，双击恢复适合整页。键盘缩放保持视口中心；`Ctrl`+滚轮或触控板捏合缩放保持指针下的位置。`v` 进入文本选择模式，拖选会高亮选区，支持反向、跨行和旋转后的文字；文本顺序取决于 PDF 的文本层。支持 PDF 文件拖入、加密文档密码输入、文档目录跳转、搜索结果高亮、书签、最近文档和阅读位置恢复。命名位置显示在侧栏，可点击返回或删除。重启后恢复页码与页内滚动，窗口尺寸变化时按相对滚动位置恢复。侧栏目录支持逐章节折叠、全部展开/折叠和 Unicode 标题筛选；筛选会显示命中条目的祖先，清空后恢复原来的折叠状态。目录支持 GoTo 动作和命名目标；内部链接可以跳转到对应页，网页链接在系统浏览器中打开。

支持 PDF 的文档页码标签，如前言 `i`、`ii` 和附录 `A-1`，同时显示对应的物理页码。按 `:` 后可以输入标签跳转；普通数字和 `42G` 始终表示物理页码，要跳到文档标签“1”可输入 `=1`。标签按区间保存，查找无需遍历整本文档。

大幅面图纸也能适合整页：缩放下限会随页面尺寸降低，低于 5% 后仍可渐进缩放；极端页面会限制最大缩放，保持界面坐标可用。

重新编译或更新 PDF 后，按 `Ctrl/Cmd+r` 可重新加载，保留页码、缩放、旋转、相对滚动位置和有效标记，并重新搜索当前查询。只有解析成功才切换文档；若文件暂时写入不完整，原文档仍可阅读，文件恢复后再次重新加载即可。

## 性能设计

- PDF 加载、渲染、文本提取和搜索均放在后台线程，UI 通过事件唤醒。
- 优先渲染当前页，再沿翻页方向优先预取相邻页；按各页尺寸、旋转和屏幕 DPI 计算分辨率，混合横竖版文档切页时可直接命中缓存。视图变化会替换尚未执行的渲染请求；已过期的在途结果会丢弃，仍需要的相同任务直接复用。
- GPU 页面纹理使用 **96 MiB LRU 缓存**，预取不会淘汰当前页；高倍缩放时跳过无法与当前页同时放入缓存的预取任务，纹理上传前再次检查容量。文本索引缓存预算 32 MiB。
- 复用文档的解释器和字形缓存；缩放时可先复用同页已有纹理。
- 搜索、整页复制与文本选择共享按需提取的文本缓存，选择模式通过共享引用接收当前页字形坐标，避免复制整页索引。
- Unicode 搜索映射每个字节只记录一个源位置，索引建成后按实际容量计入文本缓存预算。
- 按物理像素渲染，宽度以 32 像素分档；单页限制为约 1600 万像素。
- 搜索输入延迟 180 ms 合并请求，复用 Unicode 完整大小写折叠索引，支持 `Straße` / `STRASSE`、`Σ` / `σ` / `ς` 和连字，以二分定位当前页匹配与文本坐标并合并相邻高亮；结果分页面返回，最多保留 10,000 个匹配。侧栏按可见行绘制全部结果，`n` / `N` 同步定位列表；旋转、放大后也会滚到匹配处。
- 搜索尚未返回时按 `Enter` 会保留跳转请求，等结果就绪后定位；关闭搜索或主动翻页会取消待处理的跳转。
- 渲染器直接移交像素缓冲区；后台结果队列最多容纳 4 条消息，避免 UI 暂停时无限积累图像。
- 没有输入或后台结果时，界面不持续重绘。

阅读状态保存在系统应用配置目录：Linux 为 `$XDG_CONFIG_HOME/folio/library.json`（默认 `~/.config/folio/library.json`），macOS 为 `~/Library/Application Support/dev.folio.Folio/library.json`。密码不持久化。

## 验证与性能测量

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo run --release -- --render assets/folio-guide.pdf /tmp/page.png 1 1200
cargo run --release -- --text assets/folio-guide.pdf 1
cargo run --release -- --benchmark assets/folio-guide.pdf 5
# 像素旋转专项基准，交替比较原实现与当前实现
cargo bench --locked --bench rotation -- 9
# Linux 原生窗口回归测试（需要 Xvfb、libXtst 和 Python 3）
xvfb-run -a python3 scripts/smoke-desktop.py target/release/folio
xvfb-run -a python3 scripts/smoke-outline.py target/release/folio
xvfb-run -a python3 scripts/smoke-page-size.py target/release/folio
```

`--render` 的页码从 1 开始，宽度单位是像素；`--benchmark` 会渲染全部页面，输出平均数、中位数和 P95。示例 PDF 由 `python3 scripts/make-sample.py` 确定性生成，也是回归测试夹具。GitHub Actions 在 Linux 和 macOS 上执行格式检查、Clippy、测试、release 构建及命令行渲染；Linux 还执行真实桌面键盘回归测试。测量环境与结果见 [性能记录](docs/performance.md)。

扫描件可以阅读；搜索和复制依赖 PDF 文本层，目前不包含 OCR。复杂 PDF 的显示兼容性取决于 hayro。

MIT License.
