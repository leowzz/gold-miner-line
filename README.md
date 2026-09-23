# 黄金矿工辅助线

为 Gold Miner 提供固定角度参考线的桌面小工具。透明窗口覆盖游戏画面，校准后锁定，鼠标可以直接操作下方游戏。

## 使用

1. 以窗口化或无边框窗口化方式打开游戏。
2. 启动 Goldline，拖动绿色覆盖框顶部的拖动条，使边框对齐游戏画面，**不包含系统标题栏**。拖动四边或四角调整大小。
3. 把圆心拖到钩爪的悬挂轴心。此版本默认轴心在画面宽度的 50%、高度的 14%，适用于参考图所示布局，实际位置请微调。
4. 调整数量、展开角度、整体偏转、线长和样式，点击「锁定 · 开始游戏」。校准边框消失，鼠标穿透覆盖窗口。
5. 需要移动窗口或轴心时，点击「重新校准」。窗口找不到时点击「找回覆盖窗口」。控制面板同样置顶，可移到游戏旁边。

| 快捷键 | 功能 |
| --- | --- |
| Ctrl + Shift + G | 显示 / 隐藏辅助线 |
| Ctrl + Shift + L | 校准 / 锁定，并显示辅助线 |

Mac 上同样使用 Control，不是 Command。快捷键冲突时面板会提示，仍可使用按钮。关闭控制面板即退出应用。

角度以竖直向下为 0°，正值向右。线长 100% 等于覆盖框的对角线长度，超出框的部分自动裁切。展开角度是可调参考范围，不代表已经测量游戏实际摆幅。偶数条射线没有正中央的射线；奇数条时中央射线略粗、略亮。

参数和覆盖窗口位置自动保存于本机；每次启动进入校准模式。更换显示器后窗口恢复到可见区域。恢复默认参数不会改变窗口位置。

本工具不识别金块、不跟踪钩爪、不自动点击，不承诺覆盖独占全屏游戏。游戏自身移动或缩放后，请重新对齐覆盖框。

## 本地开发（Mac / Windows）

需要 Node.js 22、pnpm 11.9.0 和 Rust stable。Mac 需要 Xcode Command Line Tools；Windows 需要 Visual Studio Build Tools 的「使用 C++ 的桌面开发」、Windows SDK 和 WebView2 Runtime。

```sh
pnpm install
pnpm tauri dev
```

Mac 也可以使用 `make dev`。仅运行 `pnpm dev` 是前端开发服务器，不具备原生窗口功能。

```sh
pnpm build
pnpm test
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

## 打包

Mac：

```sh
pnpm tauri build --bundles app
```

在原生 Windows x64 环境构建 Windows 10 安装包：

```powershell
pnpm install --frozen-lockfile
pnpm tauri build --bundles nsis
```

安装包位于 `src-tauri/target/release/bundle/nsis/`。安装器在缺少 WebView2 时下载并安装运行时，需要网络连接。安装包未配置代码签名。

仓库提供 Windows GitHub Actions 工作流，可手动运行，或推送 `v*` 标签触发；生成安装包作为工作流附件，不自动发布 Release。Mac 本地构建不能证明 Windows 10 实机兼容性。

## 配置与实现

- React + TypeScript + SVG 绘制；Tauri 2 / Rust 管理透明覆盖窗口、全局快捷键与配置。
- 控制面板与覆盖层通过 Tauri 命令和带版本的状态事件同步。
- YAML 配置位于系统应用配置目录的 `com.goldline.overlay/settings.yaml`：Mac 通常在 `~/Library/Application Support/`，Windows 通常在 `%APPDATA%`。
- 参数更新约 500 ms 后原子写入，退出前刷新。读取失败提示并使用默认值；写入失败提示并自动重试。
- 窗口位置使用物理桌面坐标，尺寸使用逻辑像素，轴心按窗口比例保存。
- macOS 透明窗口使用 Tauri 的 `macOSPrivateApi`，此构建方式不面向 Mac App Store。

## 验证状态

本地验证结果见 [VERIFICATION.md](VERIFICATION.md)。Windows 10 实机、多显示器、100% / 150% 显示缩放仍需在 Windows 环境验收。
