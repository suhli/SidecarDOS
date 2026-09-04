# SidecarDOS

English | [简体中文](README.zh-CN.md)

Use your iPad as a wireless extended display for Windows, with touch, mouse, trackpad and hardware keyboard input.

## Requirements

- Windows 11 x64 with hardware H.264 encoding support and the Microsoft Visual C++ x64 runtime.
- An iPad running iPadOS 17 or later.
- Both devices connected to the same local network.

## Setup

1. On Windows, install the signed SidecarDOS display driver and configure local-network access. Follow the [Windows setup guide](docs/advanced.md#windows-setup).
2. Build and install the iPad app using a Mac with Xcode. Follow the [iPad setup guide](docs/advanced.md#ipad-setup).

If you are using the Windows developer package, follow its `INSTALL.txt` for the package-specific commands.

## Connect

1. Start `sidecardos-host.exe` on Windows as your regular logged-in user.
2. Open SidecarDOS on your iPad and allow local-network access.
3. Select your PC under **Available PCs**.
4. For the first connection, enter the pairing code shown on Windows.
5. Once connected, drag a Windows window onto the extended display.

Paired devices can reconnect without entering another code.

## Everyday use

- **Display position:** use the Windows tray menu to place the iPad to the left, right, above or below your main display.
- **Resolution and scaling:** adjust them in Windows Display Settings.
- **Quality:** choose Auto, Performance or Quality from the tray menu.
- **Controls and statistics:** open the iPad overlay to view connection information or disconnect.
- **Keyboard:** Command maps to Win; Option maps to Alt. Select matching keyboard layouts on both devices.
- **Disconnect:** use the iPad overlay or the Windows tray menu. Choose Exit in the tray to stop the Host.

## More documentation

- [Advanced guide](docs/advanced.md): building, installation, configuration, troubleshooting and architecture.
- [Development and validation status](docs/validation.md): completed checks, remaining acceptance work and current limitations.
- [Protocol specification](protocol/README.md) and [resource lifecycle](docs/lifecycle.md).
