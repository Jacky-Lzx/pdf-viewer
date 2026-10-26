<img src="assets/icon.svg" width="64" height="64" alt="Folio">

# Folio

用 Rust 编写的原生 PDF 阅读器，支持 Linux（Wayland / X11）和 macOS。使用 eframe / egui 构建界面，hayro 在后台进行纯 Rust PDF 渲染，无需安装 PDFium 或 Poppler。

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

## 键盘操作

快捷键只在阅读模式生效，输入搜索词或页码时不会触发页面导航。按 `?` 查看内置帮助。

| 按键 | 操作 |
| --- | --- |
| `j` / `k` / `h` / `l` | 向下 / 上 / 左 / 右滚动；上下到达边缘后翻页 |
| `J` / `K`，`]` / `[` | 下一页 / 上一页 |
| `gg` / `G` | 首页 / 末页 |
| `42G`，`42 Enter`，`42gg` | 跳到第 42 页 |
| `5j`，`3J` | 带数字前缀的滚动和翻页 |
| `Ctrl+d` / `Ctrl+u` | 下移 / 上移半屏 |
| `Space` / `Shift+Space` | 下移 / 上移一屏 |
| `Alt+Left` / `Alt+Right` | 返回 / 前进阅读位置历史 |
| `+` / `-` | 放大 / 缩小 |
| `p` / `w` | 适合整页 / 适合宽度 |
| `r` / `Shift+r` | 顺时针 / 逆时针旋转 |
| `/`，`Ctrl/Cmd+f` | 全文搜索 |
| `n` / `N` | 下一个 / 上一个搜索匹配 |
| `b` | 添加 / 删除当前页书签 |
| `Tab` | 打开 / 关闭侧栏 |
| `d` / `i` | 切换深色界面 / 反转页面颜色 |
| `y` | 复制当前页文本 |
| `o`，`Ctrl/Cmd+o` | 打开文档 |
| `:` | 页码跳转对话框 |
| `F11` | 全屏 |
| `Escape` | 关闭对话框或搜索，取消数字前缀 |

鼠标滚轮或触控板滚动，拖动平移，双击恢复适合整页。支持 PDF 文件拖入、加密文档密码输入、搜索结果高亮、书签、最近文档和阅读位置恢复。

## 性能设计

- PDF 加载、渲染、文本提取和搜索均放在后台线程，UI 通过事件唤醒。
- 优先渲染当前页，再预取前后相邻页；视图变化会替换尚未执行的渲染请求。
- GPU 页面纹理使用 **96 MiB LRU 缓存**；文本索引缓存预算 32 MiB。
- 复用文档的解释器和字形缓存；缩放时可先复用同页已有纹理。
- 按物理像素渲染，宽度以 32 像素分档；单页限制为约 1600 万像素。
- 搜索输入延迟 180 ms 合并请求，结果分页面返回；最多保留 10,000 个匹配。
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
```

`--render` 的页码从 1 开始，宽度单位是像素；`--benchmark` 会渲染全部页面，输出平均数、中位数和 P95。示例 PDF 由 `python3 scripts/make-sample.py` 确定性生成，也是回归测试夹具。GitHub Actions 在 Linux 和 macOS 上执行格式检查、Clippy、测试、release 构建及命令行渲染。

扫描件可以阅读；搜索和复制依赖 PDF 文本层，目前不包含 OCR。复杂 PDF 的显示兼容性取决于 hayro。

MIT License.
