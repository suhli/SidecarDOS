# Validation record

Development environment: Windows 11 x64, Rust stable 1.97.1, Visual Studio 2022 MSVC, Windows SDK 10.0.26100, official Microsoft.Windows.WDK.x64 10.0.26100.6584 package.

## Completed locally

- Rust Host and native D3D11 / Media Foundation bridge compile and link.
- Cargo formatting and strict Clippy checks.
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
