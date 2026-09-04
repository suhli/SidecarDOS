import SwiftUI
struct SettingsView: View {
    @ObservedObject var session: SessionModel
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        NavigationStack {
            Form {
                Section("Display and quality") {
                    Text("Use the SidecarDOS tray on Windows to set the display position and quality. Windows Display Settings controls resolution and UI scaling.")
                }
                Section("Input") {
                    Text("Touch sends Windows touch contacts. Hardware keyboard keys use physical positions; choose matching keyboard layouts on Windows and iPad. Command maps to Win and Option maps to Alt.")
                }
                Section("Privacy") {
                    Text("Trusted device keys are saved in this iPad’s Keychain. A new PC requires the one-time code displayed on Windows.")
                    Button("Forget current PC", role: .destructive) { session.forgetHost(); dismiss() }
                }
                Section("Connection") {
                    Text("SidecarDOS uses your local network. Allow Local Network access in iPad Settings if no PCs appear.")
                }
            }.navigationTitle("Settings").toolbar { Button("Done") { dismiss() } }
        }
    }
}
