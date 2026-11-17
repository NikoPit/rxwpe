# rxwpe

用 Rust 播放 Wallpaper Engine 项目。GPU 后端使用 wgpu，桌面输出支持 macOS AppKit，以及支持 `wlr-layer-shell` 的 Wayland 合成器。

当前已实现项目目录和 PKG 加载、静态图片场景、静音视频播放、普通窗口、Wayland 和 macOS 桌面窗口。尚未完整兼容 Wallpaper Engine。CLI 直接接受壁纸目录或 ID，不使用子命令。

## 运行

```sh
nix develop
cargo run -- examples/static-scene --window
```

作为 Wayland 桌面壁纸运行：

```sh
cargo run -- examples/static-scene --display DP-1
```

macOS 自动使用 AppKit 桌面窗口：

```sh
cargo run -- examples/static-scene --display 0
```

播放默认按操作系统选择桌面输出。Linux 使用 Wayland layer-shell，macOS 使用 AppKit。`--window` 打开普通预览窗口。`--display` 可省略，Wayland 由合成器选择输出，macOS 使用主显示器。macOS 接受显示器名称或从零开始的索引。一个进程目前只创建一个输出窗口。普通窗口可以按 Escape 退出，桌面模式可以通过 Ctrl+C 停止。

Nix 在 Linux 和 Apple Silicon macOS 提供构建与运行入口。当前锁定的 nixpkgs 不再支持 Intel macOS，Intel Mac 可以通过 Cargo 构建。

```sh
nix build
nix run . -- examples/static-scene --window
```

## 加载已有壁纸

```sh
cargo run -- /path/to/wallpaper --assets /path/to/wallpaper_engine/assets
cargo run -- 123456789 --steam-library /path/to/SteamLibrary
cargo run -- /path/to/video-wallpaper
```

数字参数表示已经下载的 Workshop ID，程序查找 `steamapps/workshop/content/431960/<ID>`。支持常见 Steam 安装目录，额外游戏库通过 `--steam-library` 指定。目前不解析 `libraryfolders.vdf`，也不下载 Workshop 内容。

资源查找顺序为项目散文件、`scene.pkg`、`gifscene.pkg`、共享 `assets` 目录。PKG 直接按索引读取，不解压到磁盘。项目资源不必全部来自官方共享目录。

通过 `--property NAME=VALUE` 覆盖项目定义的用户参数。值优先按 JSON 解析，也接受普通字符串：

```sh
cargo run -- examples/static-scene --window --property opacity=0.5
```

## Crate 职责

所有 crate 都在仓库根目录。

| Crate | 职责 |
|---|---|
| `cli` | 参数、Steam 目录定位、类型分发和应用调度 |
| `assets` | 普通目录、PKG 和共享资源的统一读取 |
| `parser` | 项目、图片场景、材质引用及纹理内容解析 |
| `scene` | scene 类型壁纸的资源实例化和画面生成 |
| `video` | video 类型壁纸的播放状态和画面生成 |
| `web` | web 类型壁纸入口，目前明确拒绝播放 |
| `shader` | shader include 展开、combo 和参数元数据读取 |
| `media` | FFmpeg 视频解码、循环和跳转 |
| `gpu` | 共享 wgpu 设备、纹理上传、图片合成和读回 |
| `output` | 普通窗口、Wayland layer-shell、macOS AppKit 桌面窗口、画面缩放和提交 |

```text
ID 或目录
    cli 定位目录
    parser + assets 加载项目
    cli 按 type 创建 scene / video / web
    播放器使用共享 GPU 生成 Frame
    output 合成到窗口并提交
```

`Frame` 持有 GPU 纹理，场景到输出不经过 CPU 像素读回。GPU crate 保留离屏读回接口供测试使用。当前视频使用 FFmpeg 子进程解码为 CPU RGBA，再上传 GPU，不含硬件解码或跨设备纹理共享。

macOS 后端复用 winit 事件循环和 wgpu Metal 输出。AppKit 窗口使用桌面层级，不拦截鼠标，并加入所有 Spaces。窗口坐标使用 NSScreen 的逻辑尺寸和位置，GPU 输出使用窗口的物理像素尺寸。Wayland 依赖仅在 Linux 编译，AppKit 依赖仅在 macOS 编译。

macOS 每秒按原显示器 ID 重新检查布局，分辨率或位置变化时重新设置窗口大小和位置。选定显示器断开时程序退出，并报告原因。

`scene`、`video` 和 `web` 不依赖 `output`。应用层把播放器回调交给 `output`，回调使用输出选择的同一个 GPU 设备。没有后台无界帧队列。

## 当前兼容范围

静态场景支持显式正交画布、图片图层、位置、缩放、平面旋转、颜色、亮度、透明度和基础透明混合。支持普通图片和部分静态 TEX，包括 RGBA、RGB、RGB565、BC1、BC2、BC3 及 LZ4 数据。

遇到已识别但尚未实现的场景行为，默认拒绝绘制，并说明原因。`--allow-partial` 可用于预览支持的图层，但会省略相应行为。

以下能力尚未实现：

- Wallpaper Engine shader 方言到 WGSL 的转换。`shader` 的预处理结果目前没有接入场景绘制，基础图片材质使用内置 WGSL。
- 多 pass 特效、SceneScript、粒子、父子变换、骨骼动画和音频响应。
- 网页浏览器及其 Wallpaper Engine JavaScript 接口。
- 视频声音播放、硬件解码、旋转元数据和 HDR。变帧率视频暂按平均帧率输出。
- 跨屏拼接、全屏应用暂停策略及 fractional-scale 协议支持。Wayland 输出支持整数缩放。

## 验证

```sh
nix develop --command cargo test --workspace
nix develop --command cargo test -p rxwpe-gpu -p rxwpe-scene -- --ignored
nix develop --command cargo clippy --workspace --all-targets -- -D warnings
nix develop --command cargo fmt --all --check
```

第二条需要可用 GPU，也可使用软件 Vulkan。它检查离屏像素方向、透明混合，以及项目加载、属性覆盖到 GPU 画面的完整过程。媒体测试会使用 FFmpeg 生成并解码短视频。

## 实现参考

文件布局及兼容行为参考以下公开实现，仓库中的实现独立编写：

- [linux-wallpaperengine](https://github.com/Almamu/linux-wallpaperengine)，项目、PKG、TEX、场景和材质加载流程。
- [WallpaperMachine](https://github.com/MMFoundation/WallpaperMachine)，播放器与平台输出的分工，以及 shader 兼容处理流程。

这些项目展示的兼容行为不等于官方引擎内部结构。本项目目前只实现上面列出的子集。
