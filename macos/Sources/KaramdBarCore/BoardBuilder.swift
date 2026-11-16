import Foundation

extension Board {
    /// Sections: each `web.today` phase, then other tasks due today or earlier, then open tasks with no phase.
    public static func build(tasks: [KTask], config: KConfig, today: Date, calendar: Calendar = .current) -> Board {
        let todayStr = Day.string(today, calendar: calendar)
        let staleCut = Day.string(Day.adding(-30, to: today, calendar: calendar), calendar: calendar)
        let open = tasks.filter(\.isOpen)
        let overdue = Set(open.filter { ($0.due ?? "9999") < todayStr }.map(\.id))
        var sections = config.today.map { p in
            TaskSection(
                title: config.phaseName(p) ?? p, phase: p,
                tasks: KTask.sorted(open.filter { $0.phase == p }, overdue: overdue))
        }
        let inPhases = Set(sections.flatMap(\.tasks).map(\.id))
        let due = open.filter { !inPhases.contains($0.id) && ($0.due ?? "9999") <= todayStr }
        sections.append(TaskSection(title: "Due", tasks: KTask.sorted(due, overdue: overdue)))
        let focus = sections.flatMap(\.tasks)
        let shown = Set(focus.map(\.id))
        let known = config.phaseKeys
        let unphased = open.filter { $0.isUnphased(known: known) && !shown.contains($0.id) }
        sections.append(TaskSection(title: "No phase", tasks: KTask.sorted(unphased, overdue: overdue)))
        return Board(
            focus: focus, sections: sections.filter { !$0.tasks.isEmpty }, overdue: overdue,
            open: open.count, stale: open.filter { ($0.createdAt ?? "9999") < staleCut }.count, error: nil)
    }

    /// The first `maxRows` tasks, keeping section order; sections left empty are dropped.
    public func capped(_ maxRows: Int) -> [TaskSection] {
        var left = maxRows
        return sections.compactMap { s in
            guard left > 0 else { return nil }
            let take = Array(s.tasks.prefix(left))
            left -= take.count
            return TaskSection(title: s.title, phase: s.phase, tasks: take)
        }
    }

    public var total: Int { sections.reduce(0) { $0 + $1.tasks.count } }
}

extension TaskSection {
    /// Every configured phase in order, with "No phase" above "next" and only when it has tasks.
    public static func byPhase(tasks: [KTask], config: KConfig, overdue: Set<String>) -> [TaskSection] {
        let known = config.phaseKeys
        var out = config.phases.map { p in
            TaskSection(
                title: p.name, phase: p.key, tasks: KTask.sorted(tasks.filter { $0.phase == p.key }, overdue: overdue))
        }
        let unphased = KTask.sorted(tasks.filter { $0.isUnphased(known: known) }, overdue: overdue)
        if !unphased.isEmpty {
            let at = config.phases.firstIndex { $0.key == "next" } ?? out.count
            out.insert(TaskSection(title: "No phase", phase: "", tasks: unphased), at: at)
        }
        return out
    }
}

extension KTask {
    /// Overdue first, then priority, then due date, then id.
    public static func sorted(_ list: [KTask], overdue: Set<String>) -> [KTask] {
        func key(_ t: KTask) -> (Int, Int, String, String) {
            (overdue.contains(t.id) ? 0 : 1, t.priorityRank, t.due ?? "9999", t.id)
        }
        return list.sorted { key($0) < key($1) }
    }

    /// No phase, or one the config does not define (removed or misspelt).
    public func isUnphased(known: Set<String>) -> Bool {
        phase.map { !known.contains($0) } ?? true
    }

    /// Case-insensitive match on title, id or any tag.
    public func matches(_ query: String) -> Bool {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return q.isEmpty || title.lowercased().contains(q) || id.contains(q)
            || (tags ?? []).contains { $0.lowercased().contains(q) }
    }
}
