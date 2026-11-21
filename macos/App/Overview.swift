import AppKit
import KaramdBarCore
import SwiftUI

/// The full task window: every open task grouped by phase, plus an editor.
@MainActor
final class Overview {
    static let shared = Overview()

    private var window: NSWindow?
    private let model = OverviewModel()

    func show(select id: String? = nil) {
        if window == nil {
            let w = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 1120, height: 740),
                styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                backing: .buffered, defer: false
            )
            w.isReleasedWhenClosed = false
            w.appearance = NSAppearance(named: .aqua)
            w.toolbarStyle = .unified
            let host = NSHostingController(
                rootView: OverviewView().environmentObject(Store.shared).environmentObject(model)
            )
            // Otherwise the window shrinks to the SwiftUI ideal size.
            host.sizingOptions = []
            host.sceneBridgingOptions = [.toolbars, .title]
            w.contentViewController = host
            w.contentMinSize = NSSize(width: 860, height: 520)
            w.setContentSize(NSSize(width: 1120, height: 740))
            w.center()
            w.setFrameAutosaveName("KaramdOverview")
            // Come to the current Space instead of switching to the window's.
            w.collectionBehavior.insert(.moveToActiveSpace)
            window = w
        }
        if let w = window, !w.isVisible { moveToMouseScreen(w) }
        installMenu()
        if let id { model.selection = id }
        NSApp.activate(ignoringOtherApps: true)
        window?.makeKeyAndOrderFront(nil)
    }

    /// Center on the screen under the mouse, keeping the saved size.
    private func moveToMouseScreen(_ w: NSWindow) {
        let mouse = NSEvent.mouseLocation
        guard let screen = NSScreen.screens.first(where: { NSMouseInRect(mouse, $0.frame, false) }),
            w.screen != screen
        else { return }
        let area = screen.visibleFrame
        var f = w.frame
        f.size.width = min(f.width, area.width)
        f.size.height = min(f.height, area.height)
        f.origin = NSPoint(x: area.midX - f.width / 2, y: area.midY - f.height / 2)
        w.setFrame(f, display: false)
    }

    /// A menu-bar-only app has no main menu, so text fields get no ⌘C/⌘V/⌘Z/⌘W.
    private func installMenu() {
        guard NSApp.mainMenu?.item(withTitle: "Edit") == nil else { return }
        let main = NSApp.mainMenu ?? NSMenu()
        func item(_ title: String, _ action: Selector, _ key: String, _ mods: NSEvent.ModifierFlags = .command)
            -> NSMenuItem
        {
            let i = NSMenuItem(title: title, action: action, keyEquivalent: key)
            i.keyEquivalentModifierMask = mods
            return i
        }
        let app = NSMenu(title: "karamd")
        app.addItem(item("Close Window", #selector(NSWindow.performClose(_:)), "w"))
        app.addItem(item("Quit", #selector(NSApplication.terminate(_:)), "q"))
        let edit = NSMenu(title: "Edit")
        edit.addItem(item("Undo", Selector(("undo:")), "z"))
        edit.addItem(item("Redo", Selector(("redo:")), "z", [.command, .shift]))
        edit.addItem(.separator())
        edit.addItem(item("Cut", #selector(NSText.cut(_:)), "x"))
        edit.addItem(item("Copy", #selector(NSText.copy(_:)), "c"))
        edit.addItem(item("Paste", #selector(NSText.paste(_:)), "v"))
        edit.addItem(item("Select All", #selector(NSText.selectAll(_:)), "a"))
        for (title, sub) in [("karamd", app), ("Edit", edit)] {
            let top = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            top.submenu = sub
            main.addItem(top)
        }
        NSApp.mainMenu = main
    }
}

@MainActor
final class OverviewModel: ObservableObject {
    @Published var selection: String?
}

// MARK: - Presentation helpers

enum Look {
    static func priorityColor(_ p: String?) -> Color {
        switch p {
        case "critical": .red
        case "high": .orange
        case "low": Color(nsColor: .tertiaryLabelColor)
        default: .secondary
        }
    }

    static func statusColor(_ s: String) -> Color {
        switch s {
        case "in-progress": .blue
        case "in-review": .purple
        case "blocked": .red
        case "completed": .green
        default: .secondary
        }
    }
}

// MARK: - Window content

struct OverviewView: View {
    @EnvironmentObject private var store: Store
    @EnvironmentObject private var model: OverviewModel
    @State private var filter = ""
    @State private var adding = false
    @State private var newTitle = ""

    var body: some View {
        NavigationSplitView {
            sidebar
                .navigationSplitViewColumnWidth(min: 280, ideal: 340, max: 460)
        } detail: {
            Group {
                if let id = model.selection {
                    TaskEditor(id: id).id(id)
                } else {
                    ContentUnavailableView(
                        "No Task Selected", systemImage: "checklist",
                        description: Text("Pick a task on the left."))
                }
            }
            .background(Color(nsColor: .textBackgroundColor))
        }
        .navigationTitle("Tasks")
        .navigationSubtitle("\(store.tasks.count) open")
        .searchable(text: $filter, placement: .sidebar, prompt: "Search")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    adding = true
                } label: {
                    Label("New Task", systemImage: "plus")
                }
                .keyboardShortcut("n")
                .help("New task (⌘N)")
                .popover(isPresented: $adding, arrowEdge: .bottom) { newTaskPopover }
            }
        }
        .overlay(alignment: .bottom) { errorBanner }
        .preferredColorScheme(.light)
    }

    private var sidebar: some View {
        List(selection: $model.selection) {
            ForEach(sections) { s in
                Section {
                    ForEach(s.tasks) { t in
                        SidebarRow(task: t, overdue: store.board.overdue.contains(t.id))
                            .tag(t.id)
                            .draggable(t.id)
                            .dropDestination(for: String.self) { ids, _ in move(ids, to: s.phase) }
                            .contextMenu { TaskMenu(task: t) }
                    }
                } header: {
                    HStack(spacing: 6) {
                        Text(s.title)
                        Text("\(s.tasks.count)").foregroundStyle(.tertiary)
                        Spacer()
                    }
                    .contentShape(Rectangle())
                    .dropDestination(for: String.self) { ids, _ in move(ids, to: s.phase) }
                }
            }
        }
        .listStyle(.sidebar)
    }

    private var newTaskPopover: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("New Task").font(.headline)
            TextField("Title", text: $newTitle)
                .textFieldStyle(.roundedBorder)
                .frame(width: 300)
                .onSubmit(create)
            Text("Goes into \(newTaskPhaseName). Press Return to add.")
                .font(.caption).foregroundStyle(.secondary)
        }
        .padding(16)
    }

    @ViewBuilder
    private var errorBanner: some View {
        if let e = store.actionError ?? store.board.error {
            HStack(spacing: 10) {
                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                Text(e).font(.callout).lineLimit(3).textSelection(.enabled)
                Spacer(minLength: 8)
                if KaramdCLI.suggestsValidate(e) {
                    Button("Run validate --fix") {
                        Task { await store.perform { try await KaramdCLI.live.validateFix() } }
                    }
                }
                Button {
                    store.actionError = nil
                } label: {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 14).padding(.vertical, 10)
            .frame(maxWidth: 560)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            .shadow(color: .black.opacity(0.12), radius: 12, y: 4)
            .padding(20)
        }
    }

    private var sections: [TaskSection] {
        let q = filter.trimmingCharacters(in: .whitespaces)
        let tasks = store.tasks.filter { $0.matches(q) }
        let all = TaskSection.byPhase(tasks: tasks, config: store.config, overdue: store.board.overdue)
        return q.isEmpty ? all : all.filter { !$0.tasks.isEmpty }
    }

    /// New tasks land in the first Today phase.
    private var newTaskPhase: String? { store.config.today.first }

    private var newTaskPhaseName: String {
        guard let p = newTaskPhase else { return "no phase" }
        return store.config.phaseName(p) ?? p
    }

    private func move(_ ids: [String], to phase: String?) -> Bool {
        guard let phase else { return false }
        Task {
            await store.perform {
                for id in ids { try await KaramdCLI.live.edit(id, "phase", phase) }
            }
        }
        return true
    }

    private func create() {
        let title = newTitle.trimmingCharacters(in: .whitespaces)
        guard !title.isEmpty else { return }
        newTitle = ""
        adding = false
        let phase = newTaskPhase
        Task {
            await store.perform {
                model.selection = try await KaramdCLI.live.create(title, phase: phase).id
            }
        }
    }
}

struct SidebarRow: View {
    let task: KTask
    let overdue: Bool

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: Vocab.statusIcon(task.status))
                .foregroundStyle(Look.statusColor(task.status))
                .font(.system(size: 13))
            Text(task.display).lineLimit(1)
            if Vocab.isUrgent(task.priority) {
                Image(systemName: Vocab.priorityIcon(task.priority))
                    .font(.system(size: 10, weight: .bold))
                    .foregroundStyle(Look.priorityColor(task.priority))
            }
            Spacer(minLength: 6)
            if let d = task.due {
                Text(Day.relative(d))
                    .font(.caption)
                    .foregroundStyle(overdue ? Color.red : Color.secondary)
            }
        }
        .padding(.vertical, 3)
    }
}

/// Context menu actions; each runs immediately.
struct TaskMenu: View {
    @EnvironmentObject private var store: Store
    let task: KTask

    var body: some View {
        Button {
            run { try await KaramdCLI.live.openSession(task.id) }
        } label: {
            Label("Open in Claude", systemImage: "terminal")
        }
        Button {
            run { try await KaramdCLI.live.postpone(task.id) }
        } label: {
            Label("Postpone to Tomorrow", systemImage: "arrow.turn.up.right")
        }
        Button {
            run { try await KaramdCLI.live.toggle(task) }
        } label: {
            Label("Complete", systemImage: "checkmark.circle")
        }
        Divider()
        Menu("Move to") {
            ForEach(store.config.phases, id: \.key) { p in
                Button(p.name) { run { try await KaramdCLI.live.edit(task.id, "phase", p.key) } }
                    .disabled(task.phase == p.key)
            }
            Button("No Phase") { run { try await KaramdCLI.live.edit(task.id, "phase", "") } }
        }
        Menu("Priority") {
            ForEach(Vocab.priorities, id: \.self) { p in
                Button(p.capitalized) { run { try await KaramdCLI.live.edit(task.id, "priority", p) } }
                    .disabled((task.priority ?? "medium") == p)
            }
        }
        Menu("Status") {
            ForEach(Vocab.statuses, id: \.self) { s in
                Button(Vocab.statusName(s)) { run { try await KaramdCLI.live.setStatus(task.id, s) } }
                    .disabled(task.status == s)
            }
        }
        Divider()
        Button("Cancel Task", role: .destructive) {
            run { try await KaramdCLI.live.setStatus(task.id, "cancelled") }
        }
    }

    private func run(_ op: @escaping () async throws -> Void) {
        Task { await store.perform(op) }
    }
}

// MARK: - Editor

/// Edits one task. Chips apply at once; title, tags and notes on ⌘S.
struct TaskEditor: View {
    @EnvironmentObject private var store: Store
    let id: String

    @State private var loaded: KTask?
    @State private var loadError: String?
    @State private var title = ""
    @State private var tags = ""
    @State private var text = ""
    @State private var pickingDate = false

    var body: some View {
        Group {
            if let t = loaded {
                content(t)
            } else if let e = loadError {
                ContentUnavailableView(
                    "Cannot Load Task", systemImage: "exclamationmark.triangle",
                    description: Text(e))
            } else {
                ProgressView().controlSize(.small)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .task { await load() }
        .onChange(of: store.tasks) { _, _ in
            Task { await load() }
        }
        .toolbar {
            if let t = loaded {
                ToolbarItemGroup(placement: .primaryAction) {
                    Button {
                        act { try await KaramdCLI.live.openSession(id) }
                    } label: {
                        Label("Open in Claude", systemImage: "terminal")
                    }
                    .help("Open in Claude")
                    Button {
                        act { try await KaramdCLI.live.postpone(id) }
                    } label: {
                        Label("Postpone", systemImage: "arrow.turn.up.right")
                    }
                    .help("Postpone to tomorrow")
                }
                ToolbarItem(placement: .primaryAction) {
                    let open = t.isOpen
                    Button {
                        toggle(t)
                    } label: {
                        Label(open ? "Complete" : "Reopen", systemImage: open ? "checkmark" : "arrow.uturn.backward")
                            .labelStyle(.titleAndIcon)
                    }
                    .keyboardShortcut(.return, modifiers: .command)
                    .help(open ? "Complete (⌘↩)" : "Reopen (⌘↩)")
                }
            }
        }
    }

    private func content(_ t: KTask) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 14) {
                TextField("Title", text: $title, axis: .vertical)
                    .textFieldStyle(.plain)
                    .font(.system(size: 24, weight: .bold))
                    .lineLimit(1...3)

                HStack(spacing: 8) {
                    statusChip(t)
                    priorityChip(t)
                    phaseChip(t)
                    dueChip(t)
                    Spacer(minLength: 0)
                }

                HStack(spacing: 6) {
                    Image(systemName: "tag").foregroundStyle(.tertiary).font(.system(size: 12))
                    TextField("Add tags, comma separated", text: $tags)
                        .textFieldStyle(.plain)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(.horizontal, 32)
            .padding(.top, 28)
            .padding(.bottom, 18)

            Divider().padding(.horizontal, 32)

            TextEditor(text: $text)
                .font(.system(size: 14))
                .lineSpacing(4)
                .scrollContentBackground(.hidden)
                .padding(.horizontal, 27)
                .padding(.top, 14)

            footer(t)
        }
    }

    private func footer(_ t: KTask) -> some View {
        HStack(spacing: 10) {
            Text("#\(t.id)").monospacedDigit()
            if let c = t.createdAt { Text("Created \(Day.relative(c))") }
            Spacer()
            if dirty {
                Text("Unsaved changes")
                Button("Revert") { fill(t) }.buttonStyle(.borderless)
                Button("Save") { Task { await save(t) } }
                    .keyboardShortcut("s")
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
            }
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .padding(.horizontal, 32)
        .padding(.vertical, 12)
        .frame(height: 44)
    }

    // MARK: Chips

    private func statusChip(_ t: KTask) -> some View {
        Menu {
            ForEach(Vocab.statuses, id: \.self) { s in
                Button {
                    act { try await KaramdCLI.live.setStatus(id, s) }
                } label: {
                    Label(Vocab.statusName(s), systemImage: Vocab.statusIcon(s))
                }
                .labelStyle(.titleAndIcon)
                .disabled(t.status == s)
            }
        } label: {
            Chip(
                icon: Vocab.statusIcon(t.status), text: Vocab.statusName(t.status),
                tint: Look.statusColor(t.status))
        }
        .chipMenu()
    }

    private func priorityChip(_ t: KTask) -> some View {
        let p = t.priority ?? "medium"
        return Menu {
            ForEach(Vocab.priorities, id: \.self) { v in
                Button(v.capitalized) { act { try await KaramdCLI.live.edit(id, "priority", v) } }
                    .disabled(p == v)
            }
        } label: {
            Chip(icon: "flag", text: p.capitalized, tint: Look.priorityColor(p))
        }
        .chipMenu()
    }

    private func phaseChip(_ t: KTask) -> some View {
        let name = store.config.phaseName(t.phase) ?? "No Phase"
        return Menu {
            ForEach(store.config.phases, id: \.key) { p in
                Button(p.name) { act { try await KaramdCLI.live.edit(id, "phase", p.key) } }
                    .disabled(t.phase == p.key)
            }
            Divider()
            Button("No Phase") { act { try await KaramdCLI.live.edit(id, "phase", "") } }
        } label: {
            Chip(icon: "square.stack", text: name)
        }
        .chipMenu()
    }

    private func dueChip(_ t: KTask) -> some View {
        let overdue = store.board.overdue.contains(t.id)
        return Button {
            pickingDate = true
        } label: {
            Chip(
                icon: "calendar", text: t.due.map { Day.relative($0) } ?? "Add Date",
                tint: overdue ? .red : (t.due == nil ? Color(nsColor: .tertiaryLabelColor) : .secondary))
        }
        .buttonStyle(.plain)
        .popover(isPresented: $pickingDate, arrowEdge: .bottom) { datePopover(t) }
    }

    private func datePopover(_ t: KTask) -> some View {
        let quick = Day.quickPicks()
        return VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 6) {
                ForEach(quick, id: \.label) { q in
                    Button(q.label) { setDue(q.date) }.controlSize(.small)
                }
            }
            DatePicker(
                "",
                selection: Binding(
                    get: { t.due.flatMap { Day.date($0) } ?? quick[0].date },
                    set: { setDue($0) }
                ), displayedComponents: .date
            )
            .datePickerStyle(.graphical)
            .labelsHidden()
            if t.due != nil {
                Button("Remove Date", role: .destructive) {
                    pickingDate = false
                    act { try await KaramdCLI.live.edit(id, "due", "") }
                }
                .buttonStyle(.borderless)
                .foregroundStyle(.red)
            }
        }
        .padding(16)
    }

    private func setDue(_ d: Date) {
        pickingDate = false
        act { try await KaramdCLI.live.edit(id, "due", Day.string(d)) }
    }

    // MARK: State

    private var dirty: Bool {
        guard let t = loaded else { return false }
        return TaskEdit.isDirty(t, title: title, tags: tags, body: text)
    }

    private func fill(_ t: KTask) {
        title = t.title
        tags = TaskEdit.formatTags(t.tags)
        text = t.body ?? ""
    }

    /// Unsaved edits survive a reload unless `refill` is set.
    private func load(refill: Bool = false) async {
        do {
            let t = try await KaramdCLI.live.show(id)
            let keepEdits = dirty && !refill
            loaded = t
            if !keepEdits { fill(t) }
            loadError = nil
        } catch {
            loadError = error.localizedDescription
        }
    }

    /// False when the edit failed.
    @discardableResult
    private func save(_ t: KTask) async -> Bool {
        let edit = TaskEdit.plan(from: t, title: title, tags: tags, body: text)
        await store.perform { try await KaramdCLI.live.edit(id, edit.fields, tags: edit.tags) }
        let ok = store.actionError == nil
        if ok && edit.clearsAllTags {
            store.actionError = "karamd cannot clear all tags yet; edit the file to remove the last tag"
        }
        await load(refill: true)
        return ok
    }

    private func act(_ op: @escaping () async throws -> Void) {
        Task {
            await store.perform(op)
            await load()
        }
    }

    /// Saves pending edits first, so ⌘↩ never drops them.
    private func toggle(_ t: KTask) {
        Task {
            if dirty, !(await save(t)) { return }
            await store.perform { try await KaramdCLI.live.toggle(t) }
            await load(refill: true)
        }
    }
}

struct Chip: View {
    let icon: String
    let text: String
    var tint: Color = .secondary

    var body: some View {
        HStack(spacing: 5) {
            Image(systemName: icon).font(.system(size: 11, weight: .semibold)).foregroundStyle(tint)
            Text(text).font(.system(size: 12, weight: .medium)).foregroundStyle(.primary.opacity(0.85))
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(Capsule().fill(Color.black.opacity(0.05)))
        .overlay(Capsule().strokeBorder(Color.black.opacity(0.06)))
        .contentShape(Capsule())
    }
}

extension View {
    func chipMenu() -> some View {
        menuStyle(.button).buttonStyle(.plain).menuIndicator(.hidden).fixedSize()
    }
}
