# Development and validation status

[User guide](../README.md) · [Advanced guide](advanced.md)

This document is the English-only record of implementation checks, acceptance gaps and current limitations. Local checks recorded below were performed on September 4, 2026.

## Overall status

The Windows Host and driver have local build results, and the hardware encoder has passed an offscreen self-test. Xcode compilation, signed driver installation and the physical Windows-to-iPad path still require validation. The full wireless extended-display MVP has not passed acceptance.

## Recorded environment

Development environment: Windows 11 x64, Rust stable 1.97.1, Visual Studio 2022 MSVC, Windows SDK 10.0.26100, official Microsoft.Windows.WDK.x64 10.0.26100.6584 package.

## Completed locally

- Rust Host and native D3D11 / Media Foundation bridge compile and link.
- Cargo formatting and strict Clippy checks.
- Seven protocol-generator regression cases: fresh LF output, CRLF checkout without rewriting, real content drift, and missing generated files for both Rust and Swift.
- Unit tests for malformed/truncated binary packets, finite numeric fields, version negotiation, replay rejection, certificate/nonce/direction binding of pairing proofs, capability-derived display modes, negative-coordinate mapping, bounded topology retries, grace reservation, physical key mapping and SPS/PPS repetition.
- QUIC integration test with a real TLS connection over loopback, fragmented reliable control writes, separate response stream direction and video Datagram fragment reassembly.
- Physical GPU self-test passed at a configured 1920×1080 / 60 FPS: creates shared GPU render targets, performs NV12 conversion, selects a hardware H.264 MFT, verifies SPS/PPS and IDR output, changes bitrate from 12 Mbps to 6 Mbps, requests another IDR and verifies it. Does not capture desktop contents or install a virtual display.
- Driver compiled with MSVC /W4 /WX against UMDF 2.25 and IddCx 1.2 headers. WDK-header-only alignment warnings are isolated from application warning checks.
- Driver DLL linked; InfVerif and Inf2Cat pass without errors/warnings; unsigned catalog generated.

The ordinary suite passed 11 unit tests and 1 QUIC integration test. The separately invoked hardware test also passed. These tests validate output and control operations; they do not establish sustained wireless 60 FPS or full-path latency. Build artifacts live under target/release and build/driver/Release and are not source-controlled.

## Requires the target environment

These checks were not executed here and remain necessary for acceptance:

- Xcode compilation of the Swift / Metal application and execution of its XCTest target.
- Apple signing and deployment to a physical iPad.
- Trust/sign/install the Windows test driver and verify IddCx monitor arrival in Windows Display Settings.
- Confirm cross-process shared texture naming / ACL access between the interactive Host and the UMDF LocalService process on the installed device.
- Drag windows to the extended desktop and measure actual 1920×1080 at 60 FPS over Wi-Fi.
- Verify VideoToolbox decode and Metal present timing with QUIC Datagram interoperability on physical iPadOS.
- Real multitouch gestures, mouse buttons, scroll, hardware keyboard modifiers/layouts and disconnect release.
- Wi-Fi outage inside/outside grace; iPad lock and background; Host/client restart; resolution/orientation change; physical monitor hotplug; GPU reset and encoder failure.
- Vendor matrix: NVIDIA / AMD / Intel. Enumeration supports all through official APIs, but one machine is not a vendor compatibility certification.

The implementation and unsigned build outputs are provided for this integration work. The repository does not claim that the full wireless extended-display MVP has passed acceptance.

## CI investigation on September 4, 2026

[GitHub Actions run 33861481873](https://github.com/suhli/SidecarDOS/actions/runs/33861481873) failed in both jobs.

- Host stopped at the generated-protocol check. The failure was reproduced locally by converting generated files to CRLF. The checker now normalizes CRLF only, generated outputs have LF Git attributes, and regression tests are part of CI and the Windows build script. The updated generator tests, protocol check, Rust formatting, Rust tests, strict Clippy and release build passed locally. This fix has not yet been verified in a new remote run.
- The iPad job reached xcodebuild and exited with code 65. Detailed compiler diagnostics were not available in this environment, so its underlying failure remains unresolved.

## Current limitations

- Supported scope is Windows 11 x64 to iPadOS, with one interactive user, one iPad and one virtual display. There is no Session 0 desktop control.
- Local driver build outputs are unsigned. They are development artifacts, not a signed driver distribution. Signing and installation are separate steps.
- Encoding requires a hardware H.264 MFT that accepts GPU input. No software fallback is implemented; incompatible hardware causes session cleanup and an explicit error.
- Vendor selection uses standard APIs, but NVIDIA, AMD and Intel hardware still need a broader compatibility test matrix.
- HEVC/AV1, HDR/10-bit, 120 Hz, Apple Pencil/PT_PEN, audio, clipboard, USB transport and multiple iPads are not implemented. Protocol fields and module boundaries allow later extensions.
- Input injection follows Windows UIPI and cannot control elevated windows, secure desktops or sign-in screens. Some iPadOS system shortcuts cannot be forwarded. Matching keyboard layouts should be selected on both devices.
- The UI consists of a basic native Windows tray and SwiftUI client. Pairing requires the full high-entropy code; there is no QR-code flow.
- iPad orientation changes reconnect and renegotiate display modes, which may briefly blank the image. Lock, background and network transitions still need physical-device acceptance.
- Latency statistics use estimated clock offsets and do not establish hardware-synchronized end-to-end latency. A successful encoder self-test does not prove sustained wireless 1080p60 performance.
- No cloud relay, Internet remote-desktop service, protected-content capture or DRM bypass is provided.
