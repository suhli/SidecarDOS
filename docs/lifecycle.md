# Resource ownership and recovery

## Driver / Host ABI

`windows/driver/Shared.h` is the native ABI contract. ABI version is 1; structs use 8-byte maximum packing. `SdStart` is 72 bytes, `SdStatus` is 80 bytes, and `SdSurfaces` is 776 bytes. The device interface GUID is 777d8d93-591a-44f4-8b3c-756e20dd9c58.

Buffered IOCTLs use FILE_DEVICE_UNKNOWN. START / STOP / SURFACES require write access; STATUS requires read access. Their CTL_CODE values are 0x22a000, 0x22a004, 0x22a00c and 0x226008 respectively. IddCx's EvtIddCxDeviceIoControl handles these, because IddCx owns the default device-control queue.

The root device is exclusive. Its ACL permits System, administrators and the interactive user. Only the per-user Host opens it. QUIC authentication is enforced in Host before arrival and input; the device interface is local OS access, not a network endpoint.

## Shared surfaces

Host allocates three shared NT-handle D3D11 textures on the render-adapter LUID reported by the driver. Names contain a random 128-bit component and use the Global SidecarDOS namespace. Security permits the owning user, LocalService (UMDF) and System. These are GPU texture handles, not a CPU pixel mapping.

- Key 0 means the driver may write. Driver uses a zero-timeout AcquireSync; if no slot is free it drops the capture.
- After GPU CopyResource / Flush, Driver releases key 1 and publishes frame ID / capture QPC timestamp under its metadata mutex.
- Host finds the newest ready slot. It releases older slots without encoding.
- Host uses key 1 while its VideoProcessor reads BGRA and produces its own NV12 texture, then releases key 0.
- NV12 surfaces are held by their Media Foundation input samples, not by shared-slot ownership. At most three input samples are outstanding.

No network operation, encode completion or indefinite keyed-mutex wait occurs inside Driver. The swapchain worker waits on compositor, shutdown and registration events. It can reuse the most recent compositor surface at the selected frame interval once a consumer has registered; without a consumer it waits indefinitely for an event. This permits initial/reconnect IDR even on an otherwise unchanged desktop.

A swapchain generation change invalidates metadata and surface registration. Registration has an additional revision so reconnecting Host resources reopen even when monitor geometry and driver generation have not changed.

## Session and cleanup

Host's capture thread owns the driver handle, COM/MFT encoder, pointer injection device and topology work. Other tasks send bounded commands and receive latest-frame / bounded status channels. Native resources do not cross Rust threads.

The state is Idle, Streaming(device), or Grace(device,deadline). Grace is configurable from 1 to 10 seconds, default 4 seconds, and reserves the monitor for the same authenticated device. The decoder and encoder restart on reconnection; only display identity and topology are retained. A new connection receives a new network token.

Normal disconnect drops the encoder and input injector immediately. All touches, keys and mouse buttons are released. Explicit stop or grace expiry departs the monitor. Closing Host's device handle triggers WDF file cleanup, so a Host crash also removes the virtual monitor. GPU/device errors stop the worker and delete the abandoned swapchain so IddCx can recreate it.

Driver worker shutdown signals an event and joins. Worker ownership is moved out of the shared mutex before joining; IddCx notifications run without the metadata mutex. COM references and shared handles use RAII. The driver only advertises one monitor.

## Reconfiguration and topology

Mode changes and a new swapchain create a new generation, then Host destroys the old MFT, creates new GPU surfaces, emits VideoConfig and requests IDR. Orientation reconnect negotiates a new capability-derived mode list, reusing monitor container identity.

Topology manager queries active and available paths, identifies SDC0001 monitor device paths, computes Extend placement against a physical anchor, validates and applies only required changes, then verifies after stabilization. It includes negative virtual desktop origins in input mapping. Three bounded attempts prevent self-triggered display-change loops.

Read-only user settings remain on disk. A manual Windows source resolution change is retained during normal reconnect. Editing the Host configuration file takes effect after Host restart; tray changes apply and persist immediately.

## Apple lifecycle

Network callbacks run on the main queue; each transport has a generation counter that rejects callbacks from canceled connections. Reliable framing retains partial messages until complete. Receive sizes, sends and frame assemblies are bounded.

VideoToolbox's asynchronous jobs retain frame context and weakly reference the decoder. Reset invalidates the decoder generation, waits for submitted callbacks, and invalidates the VT session. Metal retains CV texture views until its command buffer completes. The input UIView relinquishes held input when dismantled.

Entering inactive/background state cancels the transport and decoder work; return to foreground reconnects a trusted host. Retry delays are bounded and canceled on explicit disconnect. A fresh session resets parser, replay state, assemblies and clock-offset estimation.
