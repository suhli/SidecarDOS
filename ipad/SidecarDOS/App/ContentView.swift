import SwiftUI
struct ContentView: View {
    @ObservedObject var discovery: Discovery
    @ObservedObject var session: SessionModel
    @State private var code = ""
    @State private var overlay = false
    @State private var settings = false
    var body: some View {
        Group {
            if session.state == .streaming, let renderer = session.renderer {
                ZStack(alignment: .topTrailing) {
                    DisplaySurface(renderer: renderer, size: session.videoSize, send: session.input).ignoresSafeArea()
                    if overlay {
                        VStack(alignment: .leading, spacing: 8) {
                            Text(session.pcName).font(.headline)
                            Text("\(Int(session.videoSize.width)) × \(Int(session.videoSize.height)) · \(session.fps, specifier: "%.0f") FPS")
                            Text("Latency ≈ \(session.latencyMS, specifier: "%.1f") ms · RTT \(session.rttMS, specifier: "%.1f") ms")
                            Text("\(Double(session.bitrate)/1_000_000, specifier: "%.1f") Mbps · Loss \(session.lossPercent, specifier: "%.2f")%")
                            Text("Decode \(session.decodeMS, specifier: "%.1f") ms · Render \(session.renderMS, specifier: "%.1f") ms")
                            HStack {
                                Button("Disconnect", role: .destructive) { session.disconnect() }
                                Button("Settings") { settings = true }
                                Button("Hide") { overlay = false }
                            }
                        }.padding().background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 16)).padding()
                    } else {
                        Button { overlay = true } label: {
                            Image(systemName: "ellipsis").padding(12).background(.ultraThinMaterial, in: Circle())
                        }.accessibilityLabel("Connection controls").padding(12).opacity(0.65)
                    }
                }
            } else {
                NavigationStack {
                    List {
                        Section {
                            Label("Extend your Windows desktop to this iPad", systemImage: "display.2")
                            if !session.message.isEmpty { Text(session.message).foregroundStyle(.secondary) }
                        }
                        if session.state == .pairing {
                            Section("Pair with \(session.pcName)") {
                                TextField("Pairing code from Windows", text: $code)
                                    .textInputAutocapitalization(.never).autocorrectionDisabled()
                                    .font(.system(.body, design: .monospaced))
                                Button("Pair") { session.pair(code: code); code = "" }
                            }
                        } else if [.connecting,.reconnecting].contains(session.state) {
                            Section { ProgressView(session.state == .reconnecting ? "Reconnecting…" : "Connecting…"); Button("Cancel") { session.disconnect() } }
                        } else {
                            Section("Available PCs") {
                                ForEach(discovery.hosts) { host in
                                    Button { session.connect(host) } label: {
                                        HStack { Image(systemName: "desktopcomputer"); Text(host.name); Spacer(); Text("Online").foregroundStyle(.secondary) }
                                    }
                                }
                                if discovery.hosts.isEmpty {
                                    Text("Waiting for SidecarDOS hosts on your local network…").foregroundStyle(.secondary)
                                }
                                if let error = discovery.error { Text(error).foregroundStyle(.red) }
                            }
                            if session.state == .failed { Button("Forget this PC and pair again") { session.forgetHost() } }
                        }
                    }
                    .navigationTitle("SidecarDOS")
                    .toolbar { Button("Settings") { settings = true } }
                }
            }
        }
        .sheet(isPresented: $settings) { SettingsView(session: session) }
        .onReceive(NotificationCenter.default.publisher(for: UIDevice.orientationDidChangeNotification)) { _ in session.orientationChanged() }
        .onAppear { UIDevice.current.beginGeneratingDeviceOrientationNotifications() }
    }
}
