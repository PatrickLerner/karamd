import AppKit
import KaramdBarCore
import ServiceManagement
import SwiftUI

@main
struct KaramdBarApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @StateObject private var store = Store.shared

    var body: some Scene {
        MenuBarExtra {
            PanelView().environmentObject(store)
        } label: {
            MenuLabel(board: store.board)
        }
        .menuBarExtraStyle(.window)
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var desk: DesktopPanel?

    func applicationDidFinishLaunching(_ notification: Notification) {
        desk = DesktopPanel(store: Store.shared)
        Capture.shared.setup()
        registerLoginItem()
    }

    /// Once ever: an item the user turned off stays off, and dev builds outside an Applications folder never add one.
    private func registerLoginItem() {
        let key = "loginItemRegistered"
        guard Bundle.main.bundlePath.contains("/Applications/"), !UserDefaults.standard.bool(forKey: key) else {
            return
        }
        // register() is a blocking XPC call.
        Task.detached {
            do {
                try SMAppService.mainApp.register()
                UserDefaults.standard.set(true, forKey: key)
            } catch {
                NSLog("login item: \(error)")
            }
        }
    }
}

@MainActor
final class Store: ObservableObject {
    static let shared = Store()

    @Published private(set) var board = Board.empty
    @Published private(set) var tasks: [KTask] = []
    @Published private(set) var config = KConfig.empty
    /// Last failed write, shown in the overview until the next one succeeds.
    @Published var actionError: String?
    private var timer: Timer?
    private var watcher: VaultWatcher?
    private var refreshing = false
    private var rerun = false

    private init() {
        Task { await refresh() }
        watcher = VaultWatcher(url: Karamd.vault) {
            Task { @MainActor in await Store.shared.refresh() }
        }
        // Fallback for missed events and the date rolling over.
        timer = Timer.scheduledTimer(withTimeInterval: 300, repeats: true) { _ in
            Task { @MainActor in await Store.shared.refresh() }
        }
    }

    /// One refresh at a time; calls during a refresh queue one more, so results never arrive out of order.
    func refresh() async {
        guard !refreshing else {
            rerun = true
            return
        }
        refreshing = true
        defer { refreshing = false }
        repeat {
            rerun = false
            let (c, t, b) = await KaramdCLI.live.load()
            // On failure keep the last good tasks so the overview does not go blank.
            if b.error == nil {
                if c != config { config = c }
                if t != tasks { tasks = t }
            }
            if b != board { board = b }
        } while rerun
    }

    /// Run a write, surface its error, refresh.
    func perform(_ op: @escaping () async throws -> Void) async {
        do {
            try await op()
            actionError = nil
        } catch {
            actionError = error.localizedDescription
        }
        await refresh()
    }
}

struct MenuLabel: View {
    let board: Board

    var body: some View {
        if board.error != nil {
            Text("\(Image(systemName: "checklist")) –")
        } else if !board.overdue.isEmpty {
            Text("\(Image(systemName: "exclamationmark.circle")) \(board.focus.count)")
        } else {
            Text("\(Image(systemName: "checklist")) \(board.focus.count)")
        }
    }
}

struct PanelView: View {
    @EnvironmentObject private var store: Store
    private let maxRows = 20

    var body: some View {
        let b = store.board
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Tasks").font(.headline)
                Spacer()
                if !b.overdue.isEmpty {
                    Text("\(b.overdue.count) overdue").font(.caption).foregroundStyle(.red)
                }
            }
            if let e = b.error {
                Text("karamd failed").foregroundStyle(.secondary)
                Text(e).font(.caption).foregroundStyle(.tertiary)
            } else if b.sections.isEmpty {
                Text("Nothing open").foregroundStyle(.secondary)
            }
            SectionList(board: b, maxRows: maxRows) { t, overdue in
                TaskRow(task: t, overdue: overdue)
            }
            Divider()
            if b.error == nil {
                Text("\(b.open) open · \(b.stale) older than 30 days")
                    .font(.caption).foregroundStyle(.secondary)
            }
            HStack {
                Button("Overview ⌥⌘O") { Overview.shared.show() }
                Button("New task ⌥⌘T") { Capture.shared.show() }
                Spacer()
                Button("Refresh") { Task { await store.refresh() } }
                Button("Quit") { NSApp.terminate(nil) }
            }
        }
        .padding(12)
        .frame(width: 360)
    }
}

struct TaskRow: View {
    let task: KTask
    let overdue: Bool

    var body: some View {
        HStack(spacing: 8) {
            PriorityIcon(task: task)
            Button {
                Overview.shared.show(select: task.id)
            } label: {
                HStack {
                    Text(task.display).lineLimit(1)
                        .foregroundStyle(overdue ? Color.red : Color.primary)
                    Spacer()
                    if let d = task.due {
                        Text(d).font(.caption).foregroundStyle(.secondary)
                    }
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
    }
}
