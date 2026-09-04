import Foundation
import Network
struct DiscoveredHost: Identifiable {
    let id: String
    let name: String
    let endpoint: NWEndpoint
}
@MainActor final class Discovery: ObservableObject {
    @Published var hosts: [DiscoveredHost] = []
    @Published var error: String?
    private var browser: NWBrowser?
    func start() {
        guard browser == nil else { return }
        let browser = NWBrowser(for: .bonjour(type: "_sidecardos._udp", domain: nil), using: .udp)
        self.browser = browser
        browser.stateUpdateHandler = { [weak self] state in
            if case .failed(let error) = state { self?.error = "Local network discovery: \(error)" }
        }
        browser.browseResultsChangedHandler = { [weak self] results, _ in
            self?.hosts = results.compactMap { result in
                guard case .service(let name, let type, let domain, _) = result.endpoint else { return nil }
                return DiscoveredHost(id: "\(name).\(type).\(domain)", name: name, endpoint: result.endpoint)
            }.sorted { $0.name < $1.name }
        }
        browser.start(queue: .main)
    }
    func stop() { browser?.cancel(); browser = nil; hosts = [] }
}
