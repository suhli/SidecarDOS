# SidecarDOS

[English](README.md) | 简体中文

将 iPad 用作 Windows 的无线扩展显示器，支持触摸、鼠标、触控板和硬件键盘输入。

## 环境要求

- Windows 11 x64，支持硬件 H.264 编码，并已安装 Microsoft Visual C++ x64 运行库。
- 运行 iPadOS 17 或更新版本的 iPad。
- 两台设备连接到同一局域网。

## 安装

1. 在 Windows 上安装已签名的 SidecarDOS 显示驱动，并配置局域网访问，详见 [Windows 安装指南（英文）](docs/advanced.md#windows-setup)。
2. 使用配备 Xcode 的 Mac 构建并安装 iPad 应用，详见 [iPad 安装指南（英文）](docs/advanced.md#ipad-setup)。

如果使用 Windows 开发包，请按照包内的 `INSTALL.txt` 操作，其中的命令适用于开发包目录。

## 连接

1. 在 Windows 上，以当前普通登录用户启动 `sidecardos-host.exe`。
2. 在 iPad 上打开 SidecarDOS，允许访问本地网络。
3. 在 **Available PCs** 中选择你的电脑。
4. 首次连接时，输入 Windows 窗口中显示的配对码。
5. 连接成功后，将 Windows 窗口拖到扩展显示器上。

已配对的设备再次连接时无需重新输入配对码。

## 日常使用

- **屏幕位置：** 在 Windows 托盘菜单中，将 iPad 放在主显示器的左、右、上或下方。
- **分辨率与缩放：** 在 Windows 显示设置中调整。
- **画质：** 在托盘菜单中选择 Auto、Performance 或 Quality。
- **控制与统计：** 打开 iPad 悬浮面板，查看连接信息或断开连接。
- **键盘：** Command 对应 Win，Option 对应 Alt；两端请选择一致的键盘布局。
- **断开连接：** 使用 iPad 悬浮面板或 Windows 托盘菜单；选择托盘中的 Exit 可退出 Host。

## 更多文档

以下进阶文档使用英文：

- [进阶指南](docs/advanced.md)：构建、安装、配置、排错与架构。
- [开发与验证状态](docs/validation.md)：已完成检查、待验收项目和当前限制。
- [协议规范](protocol/README.md)与[资源生命周期](docs/lifecycle.md)。
