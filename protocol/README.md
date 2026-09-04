# SidecarDOS Protocol v1.0

Canonical source: `schema.json`. Run `node protocol/generate.mjs` after changing it; `--check` detects drift. The checked-in Rust and Swift structs and their binary readers/writers are generated together. JSON is used only as a build-time schema, never on the wire.

## Transport and framing

ALPN: `sidecardos/1`. Discovery: `_sidecardos._udp.local.`. Default UDP port: 47736.

The client opens one bidirectional reliable control stream, one unidirectional reliable input stream after authentication, and one QUIC Datagram flow. Low-rate Statistics and Ping/Pong share the control stream. Video never uses a reliable byte stream, so a lost video packet cannot block subsequent frames.

Every packet has a 36-byte header, in order:

- 4 raw bytes: ASCII `SDOS`.
- u8 major, u8 minor.
- u16 message kind.
- u32 body byte length.
- u64 reliable sequence number.
- 16 raw bytes session token.

Integers and IEEE float32 values are little endian. A `bytes` or UTF-8 `string` field is u32 length followed by the bytes. Field order is exactly the schema order. Floats must be finite. Payload readers reject trailing bytes. The session token has no length prefix in the header; schema byte fields do.

Reliable packets, including the header, must fit 65,536 bytes. Strings are limited to 1,024 UTF-8 bytes; identity fields have additional exact length checks. Receivers validate length before allocation. Datagram payloads are at most 1,200 bytes, fragment data at most 1,100 bytes, and encoded access units at most 2 MiB. Unknown messages in the active state are rejected.

The v1 header is also the bootstrap envelope for version negotiation. Handshake offers inclusive min/max major versions and a supported minor. The server selects major 1 / minor 0 or rejects; it never silently interprets a v2 packet as v1. Future incompatible field changes need a new major. Minor additions must be explicitly negotiated before use.

Sequences start at 1 and strictly increase independently on reliable control and input streams. Video uses sequence 0 and identifies data by session token, generation, frame ID and fragment index. Tokens are fresh random 128-bit values for each authenticated connection; tokens from an earlier connection are never resumed. TLS early data is disabled.

## Authentication exchange

1. Client sends Handshake with persistent 16-byte client_id, bounded name, version range, and trusted flag (0 means no saved host key, 1 means reconnect).
2. Host replies ProtocolVersion and HostInfo, including persistent 16-byte host_id.
3. Host sends Pairing: phase 0 for a new code or phase 1 for a known device, a fresh 32-byte nonce, empty proof and secret.
4. Client sends phase 2, echoes the nonce and supplies the client proof.
5. Host checks proof and sends phase 3 with its own proof. New pairing includes a fresh 32-byte device secret; reconnect uses an empty secret.
6. Client verifies the host proof before saving the certificate fingerprint and secret, then sends DisplayCapabilities.
7. Host selects modes, creates a fresh session token, and sends SessionStart under that token. EncoderCapabilities and VideoConfig follow. No display arrival or input injection is authorized before pairing succeeds.

Proof input, concatenated without length prefixes:

```text
ASCII("SidecarDOS pairing v1")
|| role:u8
|| SHA256(server_certificate_DER):32
|| client_id:16
|| nonce:32
```

Proof is HMAC-SHA256 using the one-time 16-byte pairing key or the saved 32-byte device secret. Role is 1 for client proof and 2 for host proof. The one-time code shown on Windows is the hexadecimal encoding of the key, not a six-digit PIN. The code is never sent over the connection. Certificate binding prevents an active intermediary with a different TLS certificate from relaying a usable proof. Random nonce, direction binding and fresh session token prevent reusing recorded messages.

During first pairing, iPad temporarily allows the self-signed TLS certificate solely for this exchange. Application authentication is mandatory before capabilities or streaming. On reconnect, the TLS certificate must match the pinned hash before the stream is used. Credentials are DPAPI / Keychain protected. Proof verification is constant-time through hmac / CryptoKit.

If a client has no saved key, it requests a fresh local-code pairing even if the host has an old record. This repairs interruption between host persistence and client persistence without weakening authentication.

## Video and loss recovery

Codec 1 = H.264 Annex B, bit depth 8, color space 1 = BT.709 video range. Other codec/depth values are reserved and rejected by the MVP. VideoConfig identifies a generation and actual source dimensions/fps. A new generation invalidates all assemblies and decoder output from the old generation.

VideoFragment carries generation, frame_id, capture/encode/send microsecond timestamps, keyframe flag, index/count, total access-unit bytes and fragment data. Maximum count is 4096. Repeated fragments must agree on metadata and bytes. Frames have a monotonically increasing ID within a connection; IDs count encoded output, so a transport drop is observable as a gap.

The receiver holds at most two assemblies, each for at most 40 ms. It waits up to 8 ms for normal datagram reordering before treating a frame gap as reference loss. Incomplete or missing frames trigger KeyframeRequest; decoding waits for IDR. Host repeats SPS/PPS on every IDR. Keyframe requests are limited to one per 200 ms.

The QUIC sender buffers at most four datagrams and gives each access unit a 35 ms send deadline. It never waits indefinitely for a frame to leave the network queue. Frames older than 80 ms before sending are dropped. On abandonment, the next frame is forced to IDR. Decoder and GPU submission each allow at most two in-flight frames; the renderer's pending frame is replaced by the latest arrival.

## Input and control enums

Input kind: 0 touch, 1 absolute mouse / scroll, 2 relative mouse, 3 physical keyboard. Additional kinds are reserved for future pen support.

Phase: 0 down, 1 move/update, 2 up, 3 cancellation. Touch contact IDs are 0..9; x/y are finite normalized 0..1 coordinates. Mouse buttons are a bitmask: left=1, right=2, middle=4. For kind 1, dx/dy are horizontal/vertical wheel units; for kind 2 they are relative pointer deltas.

physical_key is USB HID keyboard usage page 0x07. logical_key is a Unicode scalar hint, not text insertion. modifiers: Ctrl=1, Shift=2, Alt=4, Win/Command=8. is_repeat is 0 or 1. Host maps physical HID usages to Windows scan codes and releases all held state when a session ends.

SessionStop reason 0 is explicit disconnect; other values permit reconnect grace. KeyframeRequest reason: 0 initial, 1 configuration/reset, 2 network loss, 3 decode failure. These reason values do not bypass state checks.

## Timing and statistics

All timestamps are monotonic microseconds. Windows capture and Host timestamps share QPC; iPad uses its monotonic clock. Ping carries client t1. Pong returns t1, host receive t2 and host send t3. iPad records t4, estimates RTT as (t4-t1)-(t3-t2), and clock offset as ((t2-t1)+(t3-t4))/2, retaining the best RTT sample.

Estimated end-to-end = present_iPad + estimated_offset - capture_Windows. Decode/render durations use only local clock differences. Fragment loss is inferred from expired assemblies, not claimed to be an exact NIC packet counter. Entirely absent frames are visible through frame ID gaps; FPS and dropped frames supplement this estimate.

Adaptive bitrate reduces the target by 25% for elevated RTT, loss, decoder backlog or drops; increases by 5% only after five healthy one-second samples; clamps to configured limits. QUIC remains responsible for congestion control and cryptography.
