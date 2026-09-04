import SwiftUI
@main struct SidecarDOSApp: App {
    @StateObject private var discovery = Discovery()
    @StateObject private var session = SessionModel()
    @Environment(\.scenePhase) private var scenePhase
    var body: some Scene {
        WindowGroup {
            ContentView(discovery: discovery, session: session)
                .onAppear { discovery.start() }
                .onChange(of: scenePhase) { _, phase in
                    session.sceneActive(phase == .active)
                    if phase == .active { discovery.start() } else { discovery.stop() }
                }
        }
    }
}
