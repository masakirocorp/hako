import SwiftUI

struct ExtraSettingsView: View {
    enum Pane: String, CaseIterable, Identifiable {
        case servers
        case about

        var id: String { rawValue }

        var title: String {
            switch self {
            case .servers: "Servers"
            case .about: "About"
            }
        }
    }

    @ObservedObject var store: AgentStore
    @ObservedObject var catalog: CoordinatorCatalog
    var checkForUpdates: () -> Void
    @State private var pane = Pane.servers
    @State private var remoteTarget = ""

    var body: some View {
        Group {
            switch pane {
            case .servers:
                servers
            case .about:
                ExtraAboutView(checkForUpdates: checkForUpdates)
            }
        }
        .frame(minWidth: 520, minHeight: 400)
        .toolbar {
            ToolbarItem(placement: .principal) {
                Picker("Settings pane", selection: $pane) {
                    ForEach(Pane.allCases) { item in
                        Text(item.title).tag(item)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 220)
            }
        }
    }

    private var servers: some View {
        Form {
            Section {
                ForEach(catalog.coordinators) { coordinator in
                    HStack(alignment: .firstTextBaseline, spacing: 10) {
                        Button {
                            store.selectCoordinator(coordinator.id)
                        } label: {
                            HStack(alignment: .firstTextBaseline, spacing: 8) {
                                Image(
                                    systemName: coordinator.id == catalog.selectedId
                                        ? "checkmark.circle.fill" : "circle"
                                )
                                .foregroundStyle(
                                    coordinator.id == catalog.selectedId
                                        ? Color.accentColor : Color.secondary
                                )
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(coordinator.title)
                                    Text(coordinator.subtitle)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                        .buttonStyle(.plain)
                        Spacer(minLength: 8)
                        if coordinator.kind == .remote {
                            Button("Remove", role: .destructive) {
                                catalog.removeRemote(coordinator.id)
                                store.selectCoordinator(catalog.selectedId)
                            }
                            .buttonStyle(.borderless)
                        }
                    }
                }
            } footer: {
                Text("Gardn watches one of these servers.")
            }
            Section("Add Remote Server") {
                HStack(spacing: 8) {
                    TextField("SSH target", text: $remoteTarget, prompt: Text("user@host"))
                        .labelsHidden()
                        .textFieldStyle(.roundedBorder)
                        .onSubmit(addRemote)
                    Button("Add", action: addRemote)
                }
                if let addError = catalog.addError {
                    Text(addError)
                        .foregroundStyle(.red)
                }
            }
        }
        .formStyle(.grouped)
    }

    private func addRemote() {
        store.addRemoteCoordinator(target: remoteTarget, session: "")
        if catalog.addError == nil {
            remoteTarget = ""
        }
    }
}

struct ExtraAboutView: View {
    var checkForUpdates: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Spacer(minLength: 24)
            Image("Logo")
                .resizable()
                .interpolation(.high)
                .frame(width: 96, height: 96)
            Text("Gardn")
                .font(.system(size: 22, weight: .semibold))
            Text("Version \(version)")
                .font(.system(size: 13))
                .foregroundStyle(.secondary)
            HStack(spacing: 16) {
                Link("Website", destination: URL(string: "https://gardn.dev")!)
                Link("GitHub", destination: URL(string: "https://github.com/masakirocorp/gardn")!)
            }
            .font(.system(size: 13))
            Button("Check for Updates", action: checkForUpdates)
            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(32)
    }

    private var version: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
            ?? ""
    }
}
