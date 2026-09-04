import Foundation
import Network
import Security
import CryptoKit
@MainActor protocol SidecarTransport: AnyObject {
    func send<T: WireMessage>(_ message: T, input: Bool) throws
    func close()
}
@MainActor final class QUICTransport: SidecarTransport {
    var onPacket: ((Packet) throws -> Void)?
    var onDatagram: ((Data) throws -> Void)?
    var onReady: (() -> Void)?
    var onFailure: ((Error) -> Void)?
    private(set) var certificate = Data()
    var session = Data(repeating: 0, count: 16)
    private var group: NWConnectionGroup?
    private var control: NWConnection?
    private var input: NWConnection?
    private var datagrams: NWConnection?
    private var parser = StreamParser()
    private var controlSequence: UInt64 = 0, inputSequence: UInt64 = 0
    private var sends = 0
    private var epoch: UInt64 = 0
    func connect(_ endpoint: NWEndpoint, trusted: TrustedHost?) {
        close(); let epoch = self.epoch
        let options = NWProtocolQUIC.Options(alpn: ["sidecardos/1"])
        options.maxDatagramFrameSize = 1200
        options.idleTimeout = 6000
        sec_protocol_options_set_verify_block(options.securityProtocolOptions, { [weak self] _, trust, completion in
            guard let self, self.epoch == epoch else { completion(false); return }
            let secTrust = sec_trust_copy_ref(trust).takeRetainedValue()
            guard let chain = SecTrustCopyCertificateChain(secTrust) as? [SecCertificate],
                  let cert = chain.first else { completion(false); return }
            let der = SecCertificateCopyData(cert) as Data
            if let trusted, Data(SHA256.hash(data: der)) != trusted.certificateHash {
                completion(false); self.fail(TrustError.certificateChanged); return
            }
            self.certificate = der
            // An untrusted certificate only permits the pairing exchange; no display/input until mutual proof.
            completion(true)
        }, .main)
        let group = NWConnectionGroup(with: NWMultiplexGroup(to: endpoint), using: NWParameters(quic: options))
        self.group = group
        group.stateUpdateHandler = { [weak self] state in
            guard let self, self.epoch == epoch else { return }
            switch state {
            case .ready: self.openStreams(group, epoch: epoch)
            case .failed(let error): self.fail(error)
            case .cancelled: break
            default: break
            }
        }
        group.start(queue: .main)
    }
    private func openStreams(_ group: NWConnectionGroup, epoch: UInt64) {
        guard let control = NWConnection(from: group) else { fail(WireError.unexpected); return }
        self.control = control
        control.stateUpdateHandler = { [weak self] state in
            guard let self, self.epoch == epoch else { return }
            if case .ready = state { self.readControl(control, epoch: epoch); self.onReady?() }
            if case .failed(let error) = state { self.fail(error) }
        }
        control.start(queue: .main)
        let options = NWProtocolQUIC.Options(); options.isDatagram = true; options.maxDatagramFrameSize = 1200
        guard let flow = NWConnection(from: group, using: options) else { fail(WireError.unexpected); return }
        datagrams = flow
        flow.stateUpdateHandler = { [weak self] state in
            guard let self, self.epoch == epoch else { return }
            if case .ready = state { self.readDatagram(flow, epoch: epoch) }
            if case .failed(let error) = state { self.fail(error) }
        }
        flow.start(queue: .main)
    }
    func activateInput() {
        guard input == nil, let group else { return }
        let options = NWProtocolQUIC.Options(); options.direction = .unidirectional
        guard let stream = NWConnection(from: group, using: options) else { fail(WireError.unexpected); return }
        input = stream; stream.start(queue: .main)
    }
    private func readControl(_ stream: NWConnection, epoch: UInt64) {
        stream.receive(minimumIncompleteLength: 1, maximumLength: 16_384) { [weak self] data, _, complete, error in
            guard let self, self.epoch == epoch else { return }
            do {
                if let data { for packet in try self.parser.append(data) { try self.onPacket?(packet) } }
                if let error { throw error }
                if complete { throw WireError.unexpected }
                self.readControl(stream, epoch: epoch)
            } catch { self.fail(error) }
        }
    }
    private func readDatagram(_ flow: NWConnection, epoch: UInt64) {
        flow.receiveMessage { [weak self] data, _, _, error in
            guard let self, self.epoch == epoch else { return }
            do {
                if let error { throw error }
                if let data { guard data.count <= 1200 else { throw WireError.oversized }; try self.onDatagram?(data) }
                self.readDatagram(flow, epoch: epoch)
            } catch { self.fail(error) }
        }
    }
    func send<T: WireMessage>(_ message: T, input isInput: Bool = false) throws {
        guard let stream = isInput ? input : control else { throw WireError.unexpected }
        guard sends < 128 else { fail(WireError.oversized); throw WireError.oversized }
        if isInput { inputSequence += 1 } else { controlSequence += 1 }
        let packet = Packet(message, sequence: isInput ? inputSequence : controlSequence, session: session).encode()
        guard packet.count <= Packet.maxControl else { throw WireError.oversized }
        sends += 1; let epoch = self.epoch
        stream.send(content: packet, completion: .contentProcessed { [weak self] error in
            guard let self, self.epoch == epoch else { return }
            self.sends -= 1
            if let error { self.fail(error) }
        })
    }
    private func fail(_ error: Error) { let handler = onFailure; close(); handler?(error) }
    func closeGracefully() {
        let epoch = self.epoch
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in
            guard let self, self.epoch == epoch else {return}; self.close()
        }
    }
    func close() {
        epoch += 1; control?.cancel(); input?.cancel(); datagrams?.cancel(); group?.cancel()
        control = nil; input = nil; datagrams = nil; group = nil
        parser = StreamParser(); sends = 0; controlSequence = 0; inputSequence = 0
        session = Data(repeating: 0, count: 16); certificate = Data()
    }
}
