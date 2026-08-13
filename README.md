# Sue-Paper

Windows 原生纸张质感工具：在全部屏幕之上叠加程序生成的纸张纹理，让屏幕呈现哑光纸质感。Rust + windows-rs 直调 Win32。

## 功能

- 全屏纸张纹理覆盖层（分层窗口：逐像素半透明、鼠标键盘完全穿透、不抢焦点、不进 Alt-Tab）
- 4 种程序生成纹理（分形值噪声，feTurbulence 风格，512×512 无缝平铺）：
  Classic Matte / Whisper Weave / Sunbaked Parchment / Vellum Mist
- 强度调节 15% / 20% / 25% / 30%
- 系统托盘菜单：开关、纹理、强度、打盹（5/15 分钟）、排除当前前台应用、退出
- 内嵌 `S` 纸张纹理 Logo，无需随程序分发外部图标文件
- 应用排除列表：前台应用命中时自动隐藏纹理（500ms 轮询）
- 多显示器支持（`EnumDisplayMonitors` + `WM_DISPLAYCHANGE` 自动重建）
- 前台窗口事件监听、Z 序恢复、无闪烁覆盖层重建与单实例保护，避免切换 Electron 应用、显示变化或重复启动导致亮度跳变
- 设置持久化：`%APPDATA%/Sue-Paper/settings.json`（首次启动自动迁移旧配置）
- 资源占用：内存约 22–27 MB，运行时 0% CPU（静态纹理，仅切换时更新一次）

## 构建

需要 Rust stable 工具链（`stable-x86_64-pc-windows-gnu` 或 msvc 均可）：

```sh
./build-release.ps1
# 产物：target/release/sue-paper.exe
```

脚本会先构建程序，再把多尺寸 `S` Logo 写入 EXE 的 Windows 图标资源，使托盘、文件管理器、快捷方式和开始菜单均显示应用图标。

质量检查：

```sh
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

注意：`windows` crate 固定为 0.58（使用 `windows-targets` 预编译导入库）。
0.60+ 改用 `windows-link` 在构建期调用 dlltool 生成导入库，GNU 工具链下需额外配置 PATH，故未采用。

## 结构

| 文件 | 职责 |
|---|---|
| `src/main.rs` | 入口、隐藏 control 窗口、消息循环、定时器调度、菜单命令分发 |
| `src/overlay.rs` | 分层覆盖窗口（每屏一个）+ `UpdateLayeredWindow` 上屏 |
| `src/texture.rs` | 分形噪声纹理生成 + 4 种预设 |
| `src/tray.rs` | 托盘图标 + 右键菜单 |
| `src/exclusion.rs` | 前台进程名检测（应用排除） |
| `src/monitor.rs` | 显示器枚举 |
| `src/settings.rs` | JSON 设置持久化 |
| `assets/logo-s.png` | Logo 原图（仅含大写字母 S） |
| `assets/logo-s-64.bgra` | 编译期内嵌的托盘图标像素资源 |
| `assets/sue-paper.ico` | 文件管理器与开始菜单使用的多尺寸 Windows 图标 |
| `src/bin/embed_icon.rs` | 将 ICO 写入 release EXE 的资源工具 |

## 未实现（可作为后续迭代）

- 昼夜节律 / 定时自动开关
- 开机自启动、按显示器选择启用
