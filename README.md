<img src="assets/icon.svg" width="64" height="64" alt="Folio">

# Folio

用 Rust 编写的原生 PDF 阅读器，支持 Linux（Wayland / X11）和 macOS。使用 eframe / egui 构建界面，hayro 在后台进行纯 Rust PDF 渲染，无需安装 PDFium 或 Poppler。

<img src="assets/screenshot.png" width="1000" alt="Folio：目录筛选、缩略图、中文文件名和 Vim 链接提示">

## 运行

需要 Rust **1.92 或更新版本**。

```sh
cargo run --release -- assets/folio-guide.pdf
# 打开空白阅读器，然后选择或拖入自己的文档
cargo run --release
# 安装命令行程序
cargo install --path . --locked
folio /path/to/book.pdf
# 文件名以 - 开头时，用 -- 结束选项解析
folio -- '-notes.pdf'
folio --version
```

Linux 需要正常工作的桌面会话、OpenGL、X11 或 Wayland 及 libxkbcommon。Ubuntu / Debian 可安装：

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev libgl1-mesa-dev libx11-dev libxi-dev libxrandr-dev
```

NixOS / Nix 用户可以在 `nix-shell` 中构建和运行。Linux 文件选择器使用 XDG Desktop Portal，需要桌面环境对应的 portal 服务。macOS 使用系统文件选择器；运行 `bash scripts/bundle-macos.sh` 可把 release 构建打包为 `target/Folio.app`。

中文文件名、目录和搜索框会自动使用系统中文字体。Linux 可安装 `fonts-noto-cjk`；macOS 使用系统字体。也可以设置 `FOLIO_FONT=/path/to/font.otf` 指定回退字体，字体在后台加载。

命令行一次打开一份文档。文本导出和图像导出的页码从 1 开始，图像宽度为 1–16384 像素，基准轮数为 1–1000；缺少参数、数值越界或参数过多时，会在读取 PDF 前报告错误。Linux 和 macOS 的非 UTF-8 文件路径按原始路径保留。

## 键盘操作

页面导航快捷键只在阅读模式生效，输入搜索词或页码时不会触发页面导航。按 `?` 查看内置帮助。

| 按键 | 操作 |
| --- | --- |
| `j` / `k` / `h` / `l` | 向下 / 上 / 左 / 右滚动；上下到达边缘后翻页 |
| `J` / `K`，`]` / `[` | 下一页 / 上一页 |
| `gg` / `G` | 首页 / 末页 |
| `42G`，`42 Enter`，`42gg` | 跳到第 42 页 |
| `50%`，`25%`，`100%` | 按物理页数跳到文档的 50%、25% 或末页；`0%` 到首页 |
| `5j`，`2l`，`3J`，`3]`，`2 Space` | 带数字前缀的四向滚动、翻页和整屏滚动；数字与按键之间无需空格 |
| `Ctrl+d` / `Ctrl+u` | 下移 / 上移半屏 |
| `Ctrl+e` / `Ctrl+y`，`3 Ctrl+e` | 当前页内向下 / 向上滚动，到边缘停住；支持数字前缀 |
| `Space` / `Shift+Space` | 下移 / 上移一屏 |
| `zH` / `zL`，`2zL` | 向左 / 右滚动半屏，支持数字前缀；保留纵向位置，到边缘停住 |
| `z0` / `z$` | 当前页左端 / 右端，保留纵向位置、缩放和旋转 |
| `zt` / `zz` / `zb` | 当前页顶部 / 垂直中点 / 底部，保留缩放与横向位置 |
| 鼠标后退 / 前进侧键 | 恢复上一个 / 下一个阅读位置；文本输入和弹窗内不触发 |
| `Alt+Left` / `Alt+Right` | 历史返回 / 前进，恢复页码、缩放、旋转和滚动；支持数字前缀 |
| `+` / `-` | 放大 / 缩小 |
| `a` | 恢复 100% 缩放并保持视口中心；超大页面仍受尺寸上限约束 |
| `p` / `w` / `e` | 适合整页 / 适合宽度 / 适合高度 |
| `r` / `Shift+r`，`3r` / `3R` | 顺时针 / 逆时针旋转，支持数字前缀，回到页面起点 |
| `/`，`Ctrl/Cmd+f` | 全文搜索 |
| 搜索输入框中的 `Enter` / `Shift+Enter` | 下一个 / 上一个匹配；结果尚未就绪时等待扫描 |
| `Ctrl/Cmd+p` | 筛选最近文档；方向键或 `Ctrl+j/k` 选择，回车打开 |
| `s`，然后 `y` | 查看文档信息并复制路径、格式、大小、页数和元数据；也可点击底栏文件名打开 |
| `c` | 打开目录并筛选章节；方向键或 `Ctrl+j/k` 选择，回车跳转，`Escape` 返回阅读 |
| `Alt+c` / `Alt+w` | 搜索栏打开时切换区分大小写 / 整词匹配 |
| `n` / `N`，`3n` / `3N` | 下一个 / 上一个搜索匹配，支持数字计数（`F3` / `Shift+F3` 同样生效） |
| `f`，然后输入标签 | 跟随当前可见的 PDF 链接；`Escape` 取消 |
| `F`，然后输入标签 | 复制当前可见的网页链接地址；仅给网页链接编号，`Escape` 取消 |
| `b` | 添加 / 删除当前页书签 |
| `Alt+j` / `Alt+k`，`3 Alt+j` | 下一个 / 上一个书签，支持数字前缀；到末尾后循环 |
| `ma`，`'a` 或 `` `a `` | 保存 / 返回命名位置，字母可用 `a`–`z`；同时恢复缩放、旋转和页内滚动 |
| `Tab` | 打开 / 关闭侧栏 |
| `t` | 切换页码列表 / 缩略图，并打开侧栏 |
| `d` / `i` | 切换深色界面 / 反转页面颜色 |
| `y` | 复制当前页文本 |
| `Ctrl/Cmd+a`，然后 `y` 或 `Ctrl/Cmd+c` | 全选并复制当前页文字；搜索框等输入框内仍为全选输入文字 |
| `v`，拖选，`y` 或 `Ctrl/Cmd+C` | 选择并复制部分文本；`v` 或 `Escape` 退出选择模式 |
| 选择模式中双击 / 三击 | 选中整个词 / 整行；之后拖动按词 / 行扩展，单击恢复按字形选择 |
| `o`，`Ctrl/Cmd+o` | 打开文档 |
| `Ctrl/Cmd+r` | 重新加载当前 PDF，保留阅读位置和标记 |
| `Ctrl/Cmd+w` | 关闭当前文档，保存位置并返回欢迎页 |
| `Ctrl/Cmd+q` | 正常退出并保存最新阅读状态 |
| `:` | 页码跳转对话框 |
| `F11` | 全屏 |
| `Escape` | 关闭对话框或搜索，取消数字前缀 |

鼠标滚轮或触控板滚动，拖动平移，双击恢复适合整页。键盘缩放保持视口中心；`Ctrl`+滚轮或触控板捏合缩放保持指针下的位置。`v` 进入文本选择模式，拖选会高亮选区，支持反向、跨行和旋转后的文字；文本顺序取决于 PDF 的文本层。支持 PDF 文件拖入、加密文档密码输入、文档目录跳转、搜索结果高亮、书签、最近文档和阅读位置恢复。命名位置显示在侧栏，可点击返回或删除。重启后恢复页码与页内滚动，窗口尺寸变化时按相对滚动位置恢复。侧栏目录支持逐章节折叠、全部展开/折叠和 Unicode 标题筛选；筛选会显示命中条目的祖先，清空后恢复原来的折叠状态。目录支持 GoTo 动作和命名目标；内部链接和目录会应用 `XYZ` 的页内位置与缩放，以及 `Fit`、`FitH`、`FitV` 和 `FitR` 目标视图，支持裁剪框、文档自带旋转、阅读旋转及同页重新定位；网页链接在系统浏览器中打开；右键点击网页链接可复制地址，菜单内按回车复制、`Escape` 关闭，选择文字时保留文本操作。

支持 PDF 的文档页码标签，如前言 `i`、`ii` 和附录 `A-1`，同时显示对应的物理页码。按 `:` 后可以输入标签跳转；普通数字和 `42G` 始终表示物理页码，要跳到文档标签“1”可输入 `=1`。标签按区间保存，查找无需遍历整本文档。

百分比跳页按物理页数向上取整，例如 6 页文档的 `50%` 到第 3 页，`51%` 到第 4 页；超过 100% 到末页。跳转保留缩放和旋转，并可用历史返回；已经处于目标页时保留页内滚动。单独按 `%` 会提示先输入数字，搜索等输入框内的 `%` 仍作为文字输入。

横向图纸可按 `e` 适合高度，再用 `h/l` 横向阅读；旋转、窗口调整、标记、历史和重新打开都会保留自动适配模式。PDF 的 `FitV` / `FitBV` 目标同样使用该模式，窗口高度变化后继续适配。输入框内的字母 `e` 仍作为文字输入。

大幅面图纸也能适合整页：缩放下限会随页面尺寸降低，低于 5% 后仍可渐进缩放；极端页面会限制最大缩放，保持界面坐标可用。

侧栏 `PAGES` 可切换 `List` 页码列表和 `Previews` 缩略图，也可按 `t` 切换并打开侧栏。两种模式都会跟随键盘跳页；手动滚动侧栏后再次按 `gg` / `G`，会重新显示目标页所在行。点击跳页时保留列表的滚动位置，便于连续点击相邻页。预览支持页面旋转和颜色反转，并记住所选模式。

`v` 选择模式中双击按 Unicode 词边界选词，三击选中当前文本行；之后拖动会按相同单位扩展，单击恢复按字形选择。双击选择时保留缩放，退出选择模式后双击仍恢复适合整页。组合重音和 emoji 不会在词选择中被拆开；中文按 Unicode 默认词边界处理，整行范围取决于 PDF 提取出的文本换行。

重新编译或更新 PDF 后，按 `Ctrl/Cmd+r` 可重新加载，保留页码、缩放、旋转、相对滚动位置和有效标记，并重新搜索当前查询。只有解析成功才切换文档；若文件暂时写入不完整，原文档仍可阅读，文件恢复后再次重新加载即可。

加密文档的密码框会自动聚焦，直接键入并按回车解锁；密码错误后可继续键入重试。重新加载时若需要密码，解锁后保留当前阅读位置；按 `Escape` 取消可继续阅读已打开的文档。密码不写入阅读状态。

`Ctrl/Cmd+w` 或工具栏 Close 可在阅读、搜索、密码和打开失败的界面中关闭文档。关闭前立即提交阅读进度到后台状态写入器；界面返回欢迎页，页面和文本缓存释放，后台结束当前 PDF 会话。从最近文档重新打开会恢复位置、缩放、旋转、书签与标记。关闭空白欢迎页会保留窗口。

`Ctrl/Cmd+p` 从最近 24 份文档中按文件名或完整路径筛选，支持 Unicode 大小写折叠与空格分隔的多个关键词。方向键或 `Ctrl+j/k` 选择，回车、双击或 Open 按钮打开并恢复该文档的阅读位置。再次按快捷键、`Escape` 或 Cancel 关闭；筛选时输入不会触发页面导航。欢迎界面也提供入口。

## 性能设计

- PDF 加载、渲染、文本提取和搜索均放在后台线程，UI 通过事件唤醒。目录标题和页码前缀直接从 UTF-16 字节解码，避免为每个字符串先创建临时码元数组；普通 ASCII 标题直接复制，特殊 PDFDocEncoding 字节继续按编码表转换。
- 优先渲染当前页，再沿翻页方向优先预取相邻页；按各页尺寸、旋转和屏幕 DPI 计算分辨率，混合横竖版文档切页时可直接命中缓存。视图变化会替换尚未执行的渲染请求；已过期的在途结果会丢弃，仍需要的相同任务直接复用。
- GPU 页面纹理使用 **96 MiB LRU 缓存**，预取不会淘汰当前页；高倍缩放时跳过无法与当前页同时放入缓存的预取任务，纹理上传前再次检查容量。文本索引使用 32 MiB LRU 缓存，最多保留 512 页，容量不足时只逐出最近最少使用的页面。
- 缩略图仅请求可见行，使用独立 **8 MiB LRU 缓存**；滚动或关闭侧栏会取消过期任务，主页面渲染、文本操作和搜索优先执行。
- 复用文档的解释器和字形缓存；缩放时可先复用同页已有纹理。后台直接把 PDF 渲染到纹理像素缓冲区；0° / 180° 的反色与旋转原地完成，省去整页复制及第二块像素缓冲区；90° / 270° 直接生成旋转后的纹理，大页面使用分块访问。命令行图像导出继续在 0° 和 180° 复用原字节缓冲区。
- 搜索、整页复制与文本选择共享按需提取的文本缓存，选择模式通过共享引用接收当前页字形坐标，避免复制整页索引。
- 密集文本的拖选定位使用后台按需构建的纵向空间索引，先缩小候选范围，再按纵向、横向距离选择字形，保留短行、重叠字形及距离并列时的原有规则；索引容量计入文本缓存。整页全选先直接高亮全部文字，实际开始点击或拖选时才准备交互索引。
- 密集页的整词拖选与整词搜索共享 Unicode 边界位图，后台进入选择模式时预备，用位操作寻找相邻词边界，并按需缓存换行位置，避免反复从行首分词或扫描换行；保留组合重音、emoji、撇号和 CRLF 的选择规则。
- ASCII 使用专门的折叠快速路径；Unicode 折叠保留源字符字节边界的页面也直接使用偏移。遇到大小写展开或字节长度改变时，仅为从该字符起的后缀建立源位置映射，中文等页面可省去整个数组。索引建成后按实际容量记账，初始预留过大而未缓存的页面会重新尝试进入缓存。
- 按物理像素渲染，宽度以 32 像素分档；单页限制为约 1600 万像素。
- 搜索输入延迟 180 ms 合并请求，复用 Unicode 完整大小写折叠索引，支持 `Straße` / `STRASSE`、`Σ` / `σ` / `ς` 和连字，以二分定位当前页匹配与文本坐标并合并相邻高亮；结果分页面返回，最多保留 10,000 个匹配。侧栏按可见行绘制全部结果，`n` / `N` 同步定位列表；旋转、放大后也会滚到匹配处。
- 搜索尚未返回时按 `Enter` 会保留跳转请求，等结果就绪后定位；关闭搜索或主动翻页会取消待处理的跳转。
- 选中单行文字后按 `/`、`Ctrl/Cmd+f` 或点击 Find，会将选中文字带入搜索并保留当前搜索选项。空选区或跨行选区保留原查询，避免将换行意外放进单行输入框。
- 搜索数字前缀直接计算目标匹配，无需逐条跳转；若结果仍在扫描且目标尚未就绪，会等待足够结果再定位，扫描完成后才循环到另一端。
- 搜索摘要直接截取匹配起点前 32、后 80 个 Unicode 字符，再统一替换换行，避免临时字符数组和多次字符串拼接。
- 密集页面每批最多返回 256 条匹配，批次间优先处理新的页面渲染、文本操作和搜索请求；不再单独截断每页的第 1,000 条以后结果，统一受全局 10,000 条上限约束。
- 搜索栏的 `Aa` 和 `Word` 可分别切换区分大小写、Unicode 整词匹配，两项可组合，设置在重启后保留。查询在后台首次扫描前仅预处理一次，页面渲染和文本操作优先执行；区分大小写直接搜索原文，整词边界按需建立为每字节一位的位图并计入文本缓存。首次使用默认忽略大小写的子串搜索。中文按 Unicode 词边界处理，不进行词典分词。
- 链接提示按字母前缀直接计算匹配索引范围，最多检查三个字符；只为匹配的提示生成标签。可见链接复用索引数组，重绘时不复制链接目标，悬停后才生成说明文字。
- 链接元数据使用 **4 MiB、最多 64 页的 LRU 缓存**，计入向量和 URL 容量，容量满时逐页淘汰；过期页面结果不会挤掉当前页。超大当前页的链接临时保留在缓存之外，离开该页即释放。
- 渲染器移交像素缓冲区后，在后台转换为页面和缩略图所需的纹理像素；界面线程核对请求与缓存容量后上传。后台结果队列最多容纳 4 条消息，避免 UI 暂停时无限积累图像。
- 没有输入或后台结果时，界面不持续重绘。
- 阅读进度在独立后台线程序列化并原子替换配置文件，队列只保留最新待写快照；写入失败后显示提示并重试，正常退出会等待最新状态写完。

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
# 夜间反色与旋转：对比原两遍处理和合并处理
cargo bench --locked --bench rotation -- 9 night
# 纹理渲染：比较 RGBA 字节图像中转与直接写入纹理缓冲区，包含 PDF 光栅化
cargo bench --locked --bench texture_pixels -- 9
# 链接提示匹配基准，比较逐标签扫描与前缀范围计算
cargo bench --locked --bench link_hints -- 9
# 密集页面拖选定位，分别记录查询、建索引耗时和索引容量
cargo bench --locked --bench selection -- 9
# 整词范围查询，比较重复分词与共享边界位图
cargo bench --locked --bench word_selection -- 9
# 搜索索引构建，比较原映射与 ASCII 直接偏移的耗时及容量
cargo bench --locked --bench search_index -- 9
# 搜索摘要，比较临时字符串拼接与直接截取上下文
cargo bench --locked --bench search_snippet -- 9
# PDF 标题解码，比较 UTF-16 直接解码和 ASCII 复制路径
cargo bench --locked --bench pdf_strings -- 9
# Linux 原生窗口回归测试（需要 Xvfb、libXtst 和 Python 3）
xvfb-run -a python3 scripts/smoke-desktop.py target/release/folio
xvfb-run -a python3 scripts/smoke-cli.py target/release/folio
xvfb-run -a python3 scripts/smoke-outline.py target/release/folio
xvfb-run -a python3 scripts/smoke-outline-keyboard.py target/release/folio
xvfb-run -a python3 scripts/smoke-bookmarks.py target/release/folio
xvfb-run -a python3 scripts/smoke-page-size.py target/release/folio
xvfb-run -a python3 scripts/smoke-fit-height.py target/release/folio
xvfb-run -a python3 scripts/smoke-destinations.py target/release/folio
xvfb-run -a python3 scripts/smoke-thumbnails.py target/release/folio
xvfb-run -a python3 scripts/smoke-link-hints.py target/release/folio
xvfb-run -a python3 scripts/smoke-link-menu.py target/release/folio
xvfb-run -a python3 scripts/smoke-link-copy.py target/release/folio
xvfb-run -a python3 scripts/smoke-selection.py target/release/folio
xvfb-run -a python3 scripts/smoke-search-load.py target/release/folio
xvfb-run -a python3 scripts/smoke-search-options.py target/release/folio
xvfb-run -a python3 scripts/smoke-search-dense.py target/release/folio
xvfb-run -a python3 scripts/smoke-persistence.py target/release/folio
xvfb-run -a python3 scripts/smoke-recent.py target/release/folio
xvfb-run -a python3 scripts/smoke-close.py target/release/folio
xvfb-run -a python3 scripts/smoke-properties.py target/release/folio
xvfb-run -a python3 scripts/smoke-input-burst.py target/release/folio
xvfb-run -a python3 scripts/smoke-mouse-input.py target/release/folio
xvfb-run -a python3 scripts/smoke-mouse-history.py target/release/folio
xvfb-run -a python3 scripts/smoke-password.py target/release/folio
xvfb-run -a python3 scripts/smoke-vim-view.py target/release/folio
xvfb-run -a python3 scripts/smoke-percentage.py target/release/folio
```

`--render` 的页码从 1 开始，宽度单位是像素；`--benchmark` 会渲染全部页面，输出平均数、中位数和 P95。示例 PDF 由 `python3 scripts/make-sample.py` 确定性生成，也是回归测试夹具。GitHub Actions 在 Linux 和 macOS 上执行格式检查、Clippy、测试、release 构建及命令行渲染；Linux 还执行真实桌面键盘回归测试。测量环境与结果见 [性能记录](docs/performance.md)。

扫描件可以阅读；搜索和复制依赖 PDF 文本层，目前不包含 OCR。复杂 PDF 的显示兼容性取决于 hayro。

MIT License.
