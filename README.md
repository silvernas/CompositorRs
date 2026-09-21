# Compositor（Windows 版）

Compositor 是一款免费、开源的 Photoshop 风格图像编辑器。本仓库是使用 **Rust + egui (eframe)** 编写的 **Windows 原生**移植版，源项目是Compositor 为robbietilton使用swift实现的基于MacOS平台运行版本，出于在Windows平台使用的考虑而创建了本项目。和源项目一样：围绕合成与后期处理工作流，提供制作像素级精确成品图像所需的工具。

当前版本为 **0.1.0**，界面为简体中文，深色主题。全部像素处理在 CPU 上完成（rayon 并行），复用 8 个 C 像素内核（通过 FFI 调用），不依赖 GPU。

## 功能特性

以下列表仅包含**当前已实现**的功能，与代码实际能力一一对应。

### 文件与文档
- 新建文档（自定义宽度/高度，1–30000 校验）
- 多文档标签页管理，每个文档独立的撤销/重做历史（快照式，自动裁剪内存预算）
- 打开图像：**PNG / JPEG / TIFF**（[`image`](https://crates.io/crates/image)）；**HEIC/HEIF** 需启用 `heic` feature（见下文）
- 保存为 PNG；导出为 **PNG / JPEG**（JPEG 质量可调，1–100）

### 图层
- 新建、复制、删除、上移/下移排序、可见性切换、不透明度
- **14 种混合模式**（Normal、Multiply、Screen、Overlay、Soft Light、Darken、Lighten、Difference、Color Dodge、Color Burn、Hue、Saturation、Color、Luminosity，按 W3C Compositing & Blending 公式实现）
- **调整图层**（非破坏性，作用于其下所有图层）：色阶、曲线、色相/饱和度、曝光、渐变映射、颗粒
- **图层样式**（像素图层）：描边、投影、颜色叠加、内阴影
- 合并向下（Ctrl+E）
- 自由变换（Ctrl+T）：平移/缩放/旋转/水平垂直翻转，Enter 提交、Esc 取消；方向键微调位置

### 绘画与文字
- 画笔：软圆笔刷，支持大小、硬度、不透明度与颜色；`[` / `]` 调整大小，按住 Shift 绘制直线
- 文字工具（T）：点击画布定位，输入多行文本（字号、颜色可调），`ab_glyph` 光栅化为像素图层，支持中文

### 选区
- 矩形选框（M）、魔棒（W，容差可调，Shift 加选 / Alt 减选）
- 全选（Ctrl+A）、取消选择（Ctrl+D）
- 删除选区内容（Del）、填充前景色（Shift+F5）
- **内容识别填充选区**、**移除背景**（自动采样四角背景并删除）
- 选区限定作用于破坏性调整（如色相/饱和度，按覆盖度混合）

### 调整与滤镜
- 色相/饱和度（对话框：色相/饱和度/明度/着色，实时预览开关，重置/确定/取消，仅作用于全尺寸像素图层）
- 高斯模糊（对话框，半径 0.5–100px，破坏性、可撤销）
- 色阶 / 曲线 / 曝光 / 渐变映射 / 颗粒（作为调整图层参数化编辑，含交互式曲线编辑器：点击加点、拖动调整、双击删点，支持 RGB/R/G/B 通道）

### 画布与显示
- 缩放 2%–6400%（滚轮/触控板，以指针为中心），Space 或中键平移画布
- 适合窗口 / 100% 缩放（双击画布适合窗口）
- 缩小时面积平均下采样（4096px 上限），放大 ≥6 倍时显示像素网格
- 棋盘格透明背景

### 快捷键
Ctrl+N / Ctrl+O / Ctrl+S、Ctrl+Z / Ctrl+Shift+Z、Ctrl+A / Ctrl+D、Ctrl+T（自由变换）、Ctrl+E（合并向下）、Del、Shift+F5（填充前景色）、B/M/V/W/T（工具切换）、`[`/`]`（画笔大小）、方向键微调图层。

## 环境要求

- Windows 10 或更新版本（64 位）
- Rust 工具链（stable，`rustup.rs` 安装）——仅从源码构建时需要
- （可选）启用 `heic` feature 时，需要系统安装 libheif 库

## 构建

```powershell
cargo build --release
```

生成的程序位于 `target\release\compositor.exe`。

启用 HEIC/HEIF 导入（需要系统 libheif 库）：

```powershell
cargo build --release --features heic
```

## 发布

`scripts\package-windows.ps1` 会依次执行：生成程序图标（如缺失）→ Release 构建 → 按可用工具生成安装包：

1. **Inno Setup 6**（优先）：产出 `dist\Compositor-Setup-<版本号>.exe`（安装界面含简体中文）
2. **WiX toolset + cargo-wix**（备选）：产出 MSI 安装包
3. **便携 zip**（兜底）：产出 `dist\Compositor-win64.zip`

```powershell
powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1
```

安装 Inno Setup：<https://jrsoftware.org/isdl.php>；安装 WiX：

```powershell
winget install --id WiXToolset.WiXToolset.31
cargo install cargo-wix
```

## 架构概览

- `src/app.rs` — 主应用状态、菜单、对话框与工具逻辑
- `src/ui/` — 工具栏、图层面板、画布交互
- `src/core/` — 文档、图层、选区、混合模式、历史记录（快照撤销）
- `src/rendering/` — CPU 合成器、滤镜、调整图层、图层样式、缩放下采样、文字光栅化
- `src/io/` — 图像导入/导出
- `src/c/` — 8 个 C 像素内核（画笔、污点修复、色阶、魔棒、杂色、镜头校正、内容填充、调整），经 `src/ffi.rs` 调用

## 与 macOS 版的关系

仓库根目录另有一份基于 Swift/AppKit（Xcode）的 macOS 实现。本目录（`compositor-rs`）是独立的 Rust/egui 重实现，功能仍在持续对齐中——macOS 版的部分功能（图层蒙版、剪贴蒙版、套索、渐变/形状工具、仿制图章、画布/图像大小等）尚未移植到本版本。

## 许可证

MIT 许可证——详见仓库根目录的 [LICENSE](../LICENSE)。
