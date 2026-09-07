# Development and validation status

[User guide](../README.md) · [Advanced guide](advanced.md)

This document is the English-only record of implementation checks, acceptance gaps and current limitations. Local build checks below were performed on September 4, 2026; the latest remote CI status was inspected on September 7, 2026.

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

- Host stopped at the generated-protocol check. The failure was reproduced locally by converting generated files to CRLF. The checker now normalizes CRLF only, generated outputs have LF Git attributes, and regression tests are part of CI and the Windows build script. The updated generator tests, protocol check, Rust formatting, Rust tests, strict Clippy and release build passed locally. The Host job subsequently passed in run 33863256206 at commit 4c638d4e17e52c3c8e8b9f58ab7461e0eb6b62fb.
- The iPad job reached Xcode 16.4 / iOS Simulator SDK 18.5 and exited with code 65. The supplied log identified two initializer conflicts on both arm64 and x86_64: a throwing DisplayRenderer.init() overriding non-throwing NSObject.init(), and a failable InputView.init(coder:) overriding the non-failable MTKView initializer. The renderer now uses a throwing makeDefault() factory and an explicit init(device:); the unavailable coder initializer matches the superclass and delegates to it. Source and call sites were reviewed locally. Xcode is unavailable on this Windows machine, so the corrected iPad build still requires a new CI run.

- A subsequent supplied iPad log passed the initializer declarations and reported MainActor isolation errors in QUIC Sendable state callbacks, plus matching warnings in receive/send completions. All seven QUIC/TLS callback boundaries now explicitly use MainActor.assumeIsolated under their configured main-queue contract. Bonjour callbacks use the same pattern with stale-generation rejection; connection state handlers no longer retain their owning connections. Concurrency checking remains enabled. Xcode verification is still pending because this environment is Windows. The iPad workflow now retains the complete build log and xcresult bundle as a failure artifact.

## CI status on September 7, 2026

An earlier check found that [Actions run 33863256206](https://github.com/suhli/SidecarDOS/actions/runs/33863256206) built commit 4c638d4e17e52c3c8e8b9f58ab7461e0eb6b62fb: Host passed and iPad failed. At that point, the MainActor callback corrections had not been committed. Those changes are now included in commit b555fb3.

The subsequent supplied Xcode 16.4 log includes the updated diagnostic workflow and no longer reports the Transport.swift isolation errors. It fails on DisplayRenderer.swift because the Simulator target does not expose addPresentedHandler, and warns about reading a non-Sendable VTDecompressionSession from H264Decoder's nonisolated deinit.

- Simulator builds now exclude drawable presentation callbacks and estimate timing using successful GPU command-buffer completion. Physical iPad builds retain actual drawable presentation timing.
- A private session owner drains and invalidates the VideoToolbox session when released, including during decoder reset, reconfiguration and destruction. No concurrency checks are disabled or native session Sendable conformance added.
- The iPad CI job now builds both Simulator (including the XCTest target) and the unsigned iPhoneOS app so both conditional rendering paths are compiled. Each target has a separate diagnostic artifact.

The revised Swift source has been reviewed locally, but this Windows environment cannot compile either Apple SDK target. A new CI run is required to validate these changes.

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
