import Foundation
import UIKit
import CryptoKit
import os

@MainActor final class SessionModel: ObservableObject {
    enum State: Equatable { case idle, connecting, pairing, streaming, reconnecting, failed }
    @Published var state: State = .idle
    @Published var message = ""
    @Published var pcName = ""
    @Published var videoSize = CGSize(width: 1920, height: 1080)
    @Published var fps: Float = 0
    @Published var bitrate: UInt32 = 0
    @Published var rttMS: Double = 0
    @Published var latencyMS: Double = 0
    @Published var decodeMS: Double = 0
    @Published var renderMS: Double = 0
    @Published var lossPercent: Double = 0
    let renderer: DisplayRenderer?
    private let transport = QUICTransport()
    private let decoder = H264Decoder()
    private var assembler = FrameAssembler()
    private var host: DiscoveredHost?
    private var trusted: TrustedHost?
    private var clientID = Data()
    private var hostID = Data()
    private var nonce = Data(), pairKey = Data()
    private var generation: UInt32?
    private var lastSequence: UInt64 = 0
    private var pairingNew = false
    private var configured = false, wantsConnection = false, foreground = true
    private var clockOffset: Double?
    private var bestRTT = Double.greatestFiniteMagnitude
    private var reconnectAttempt = 0
    private var reconnectTask: Task<Void, Never>?
    private var timeoutTask: Task<Void, Never>?
    private var timer: Timer?
    private var lastStatistics = monotonicUS(), lastKeyframe: UInt64 = 0
    private var presented: UInt32 = 0, receivedBytes: UInt64 = 0, intervalBytes: UInt64 = 0
    private var previousLosses: UInt32 = 0, previousReceived: UInt32 = 0, previousMissing: UInt32 = 0
    private let log = Logger(subsystem: "dev.sidecardos", category: "session")
    init() {
        do { clientID = try TrustStore.identity(); renderer = try DisplayRenderer() }
        catch { renderer = nil; message = "Initialization failed: \(error)"; state = .failed }
        transport.onReady = { [weak self] in self?.ready() }
        transport.onPacket = { [weak self] packet in try self?.packet(packet) }
        transport.onDatagram = { [weak self] bytes in try self?.datagram(bytes) }
        transport.onFailure = { [weak self] error in self?.failed(error) }
        decoder.onFrame = { [weak self] frame in self?.renderer?.enqueue(frame) }
        decoder.onResetNeeded = { [weak self] in self?.requestKeyframe(reason: 3) }
        renderer?.onPresent = { [weak self] frame, start, end in self?.present(frame, start: start, end: end) }
    }
    func connect(_ host: DiscoveredHost) {
        disconnect(); guard renderer != nil, clientID.count == 16 else { return }
        self.host = host; pcName = host.name; wantsConnection = true; foreground = true; reconnectAttempt = 0
        open()
    }
    private func open() {
        guard wantsConnection, foreground, let host else { return }
        do { trusted = try TrustStore.host(host.id) }
        catch { failed(error); return }
        configured = false; generation = nil; lastSequence = 0
        assembler = FrameAssembler(); decoder.reset(); renderer?.reset()
        clockOffset = nil; bestRTT = .greatestFiniteMagnitude
        previousLosses = 0; previousReceived = 0; previousMissing = 0
        lastStatistics = monotonicUS(); intervalBytes = 0; presented = 0
        state = reconnectAttempt == 0 ? .connecting : .reconnecting
        message = "Connecting to \(host.name)…"
        transport.connect(host.endpoint, trusted: trusted)
        timeoutTask?.cancel()
        timeoutTask = Task { [weak self] in
            do { try await Task.sleep(for: .seconds(100)) } catch { return }
            guard let self, self.state != .streaming else { return }
            self.failed(WireError.unexpected)
        }
    }
    private func ready() {
        do { try transport.send(Handshake(minMajor: 1, maxMajor: 1, minor: 0, clientId: clientID, name: UIDevice.current.name, trusted: trusted == nil ? 0 : 1)) }
        catch { failed(error) }
    }
    func pair(code: String) {
        do { pairKey = try TrustStore.code(code); try sendProof(); message = "Verifying PC…" }
        catch { message = "Enter the 32-character code displayed by SidecarDOS on your PC." }
    }
    private func sendProof() throws {
        guard nonce.count == 32, !transport.certificate.isEmpty else { throw WireError.unexpected }
        let proof = TrustStore.proof(key: pairKey, certificate: transport.certificate, client: clientID, nonce: nonce, role: 1)
        try transport.send(Pairing(phase: 2, nonce: nonce, proof: proof, secret: Data()))
    }
    private func packet(_ p: Packet) throws {
        guard p.sequence > lastSequence else { throw WireError.replay }
        lastSequence = p.sequence
        if p.kind == SessionStart.kind {
            guard configured, p.session.count == 16, p.session != Data(repeating: 0, count: 16) else { throw WireError.unexpected }
            transport.session = p.session; transport.activateInput()
        } else if p.session != transport.session { throw WireError.replay }
        switch p.kind {
        case ProtocolVersion.kind:
            let version = try p.message(ProtocolVersion.self)
            guard version.major == 1 else { throw WireError.version }
        case HostInfo.kind:
            let info = try p.message(HostInfo.self); guard info.hostId.count == 16 else { throw WireError.malformed }
            hostID = info.hostId; pcName = info.name
        case Pairing.kind:
            let pair = try p.message(Pairing.self)
            guard pair.nonce.count == 32 else { throw WireError.malformed }
            switch pair.phase {
            case 0:
                pairingNew = true
                nonce = pair.nonce; state = .pairing
                message = "Enter the one-time pairing code shown on \(pcName)."
            case 1:
                guard let trusted, trusted.hostID == hostID else { throw TrustError.invalidProof }
                pairingNew = false; nonce = pair.nonce; pairKey = trusted.secret; try sendProof()
            case 3:
                guard pair.nonce == nonce, let host,
                      TrustStore.verify(pair.proof, key: pairKey, certificate: transport.certificate, client: clientID, nonce: nonce, role: 2)
                else { throw TrustError.invalidProof }
                if pairingNew {
                    guard pair.secret.count == 32 else { throw TrustError.invalidProof }
                    let saved = TrustedHost(certificateHash: Data(SHA256.hash(data: transport.certificate)), secret: pair.secret, hostID: hostID)
                    try TrustStore.save(host.id, saved); trusted = saved
                }
                pairKey = Data(); configured = true; state = .connecting
                try transport.send(Capabilities.current())
            default: throw WireError.unexpected
            }
        case SessionStart.kind:
            let start = try p.message(SessionStart.self)
            guard start.width >= 320, start.width <= 4094, start.height >= 320, start.height <= 4094 else { throw WireError.malformed }
            videoSize = CGSize(width: Int(start.width), height: Int(start.height))
            message = "Starting virtual display…"
        case EncoderCapabilities.kind:
            let caps = try p.message(EncoderCapabilities.self)
            guard caps.codecs & 1 == 1, caps.hardware == 1 else { throw WireError.unexpected }
        case VideoConfig.kind:
            let config = try p.message(VideoConfig.self)
            guard configured, config.codec == 1, config.bitDepth == 8, config.colorSpace == 1,
                  config.width >= 320, config.width <= 4094, config.height >= 320, config.height <= 4094,
                  config.width % 2 == 0, config.height % 2 == 0, (30...60).contains(config.fps) else { throw WireError.malformed }
            generation = config.generation; videoSize = CGSize(width: Int(config.width), height: Int(config.height))
            assembler = FrameAssembler(); decoder.reset(); renderer?.reset()
            previousLosses = 0; previousReceived = 0; previousMissing = 0
            state = .streaming; message = ""; reconnectAttempt = 0; timeoutTask?.cancel()
            UIApplication.shared.isIdleTimerDisabled = true
            startTimer(); requestKeyframe(reason: 0)
        case Pong.kind:
            let pong = try p.message(Pong.self); let now = monotonicUS()
            guard now >= pong.clientTimestamp, pong.hostSend >= pong.hostReceive else { throw WireError.malformed }
            let rtt = Double(now - pong.clientTimestamp) - Double(pong.hostSend - pong.hostReceive)
            guard rtt >= 0 else { return }
            rttMS = rtt / 1000
            if rtt < bestRTT {
                bestRTT = rtt
                clockOffset = ((Double(pong.hostReceive) - Double(pong.clientTimestamp)) + (Double(pong.hostSend) - Double(now))) / 2
            }
        case ProtocolError.kind:
            let e = try p.message(ProtocolError.self)
            throw NSError(domain: "SidecarDOS", code: Int(e.code), userInfo: [NSLocalizedDescriptionKey: e.message])
        default: throw WireError.unexpected
        }
    }
    private func datagram(_ data: Data) throws {
        guard state == .streaming else { return }
        let p = try Packet(data: data, limit: 1200)
        guard p.session == transport.session else { return }
        let fragment = try p.message(VideoFragment.self)
        guard fragment.generation == generation else { return }
        receivedBytes &+= UInt64(data.count); intervalBytes &+= UInt64(data.count)
        try assembler.ingest(fragment, now: monotonicUS())
        consumeFrames()
    }
    private func consumeFrames() {
        let now = monotonicUS(); assembler.expire(now)
        while let frame = assembler.next(now: now) { decoder.decode(frame) }
        if assembler.requiresKeyframe { requestKeyframe(reason: 2) }
    }
    private func requestKeyframe(reason: UInt8) {
        guard state == .streaming else { return }
        let now = monotonicUS(); guard now - lastKeyframe >= 200_000 else { return }
        lastKeyframe = now
        do { try transport.send(KeyframeRequest(reason: reason)) } catch { failed(error) }
    }
    func input(_ e: InputEvent) {
        guard state == .streaming else { return }
        do { try transport.send(e, input: true) } catch { failed(error) }
    }
    private func startTimer() {
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 0.01, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
    }
    private func tick() {
        guard state == .streaming else { return }
        consumeFrames()
        let now = monotonicUS(); let elapsed = now - lastStatistics
        guard elapsed >= 1_000_000 else { return }
        fps = Float(presented) * 1_000_000 / Float(elapsed)
        bitrate = UInt32(clamping: intervalBytes * 8 * 1_000_000 / elapsed)
        let received = assembler.receivedFragments &- previousReceived
        let missing = assembler.missingFragments &- previousMissing
        let total = UInt64(received) + UInt64(missing)
        let loss = total > 0 ? UInt32(UInt64(missing) * 1_000_000 / total) : 0
        lossPercent = Double(loss) / 10_000
        let dropped = assembler.losses &- previousLosses
        previousLosses = assembler.losses; previousReceived = assembler.receivedFragments; previousMissing = assembler.missingFragments
        do {
            try transport.send(Ping(clientTimestamp: now))
            try transport.send(Statistics(fps: fps, bitrate: bitrate, rttUs: UInt32(clamping: Int64(rttMS * 1000)),
                lossPpm: loss, decodeQueue: UInt16(decoder.inFlight), droppedFrames: dropped, receivedBytes: receivedBytes,
                decodeUs: UInt32(clamping: Int64(decodeMS * 1000)), renderUs: UInt32(clamping: Int64(renderMS * 1000)),
                estimatedLatencyUs: UInt32(clamping: Int64(latencyMS * 1000))))
        } catch { failed(error) }
        lastStatistics = now; intervalBytes = 0; presented = 0
    }
    private func present(_ frame: DecodedFrame, start: UInt64, end: UInt64) {
        guard state == .streaming, frame.received.video.generation == generation else { return }
        presented &+= 1
        decodeMS = Double(frame.decodeEnd - frame.decodeStart) / 1000
        renderMS = Double(end > start ? end - start : 0) / 1000
        if let clockOffset {
            latencyMS = max(0, (Double(end) + clockOffset - Double(frame.received.video.captureTimestamp)) / 1000)
        }
    }
    func orientationChanged() {
        guard state == .streaming else { return }
        // Existing advertised modes remain valid; the Windows source rotates without recreating device identity.
        let cap = Capabilities.current()
        let landscape = cap.orientation == 1
        let currentlyLandscape = videoSize.width > videoSize.height
        if landscape != currentlyLandscape {
            // Renegotiate capabilities on a fresh session; reconnect grace retains the monitor until replacement modes arrive.
            failed(NSError(domain: "SidecarDOS", code: 1, userInfo: [NSLocalizedDescriptionKey: "Updating orientation"]))
        }
    }
    func sceneActive(_ active: Bool) {
        foreground = active
        if active {
            if wantsConnection, state != .streaming, state != .connecting, state != .pairing { reconnectAttempt = 0; open() }
        } else {
            reconnectTask?.cancel(); timeoutTask?.cancel(); timer?.invalidate()
            transport.close(); decoder.reset(); renderer?.reset(); UIApplication.shared.isIdleTimerDisabled = false
            if wantsConnection { state = .reconnecting; message = "Connection paused" }
        }
    }
    private func failed(_ error: Error) {
        if !wantsConnection {transport.close(); state = .idle; return}
        log.error("Session stopped: \(String(describing: error), privacy: .public)")
        transport.close(); decoder.reset(); renderer?.reset(); timer?.invalidate(); timeoutTask?.cancel()
        UIApplication.shared.isIdleTimerDisabled = false
        message = error.localizedDescription
        guard wantsConnection, foreground, trusted != nil,
              !(error is TrustError) else { wantsConnection = false; state = .failed; return }
        state = .reconnecting; reconnectAttempt += 1
        let delay = min(3.0, 0.2 * pow(2, Double(min(reconnectAttempt-1, 4))))
        reconnectTask?.cancel()
        reconnectTask = Task { [weak self] in
            do { try await Task.sleep(for: .seconds(delay)) } catch { return }
            self?.open()
        }
    }
    func disconnect() {
        wantsConnection = false; reconnectTask?.cancel(); timeoutTask?.cancel(); timer?.invalidate()
        if state == .streaming { try? transport.send(SessionStop(reason: 0)) }
        transport.closeGracefully(); decoder.reset(); renderer?.reset(); state = .idle
        UIApplication.shared.isIdleTimerDisabled = false
    }
    func forgetHost() { if let host { TrustStore.forget(host.id) }; disconnect(); trusted = nil }
}
