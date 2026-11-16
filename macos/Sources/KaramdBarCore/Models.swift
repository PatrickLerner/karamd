import Foundation

/// One task as `karamd list|show|create --json` prints it (`TaskView`). Unused fields are ignored.
public struct KTask: Decodable, Identifiable, Hashable, Sendable {
    public let id: String
    public let title: String
    public let status: String
    public let priority: String?
    public let phase: String?
    public let due: String?
    public let createdAt: String?
    public var tags: [String]?
    /// Only `show` and `create` include it.
    public var body: String?

    enum CodingKeys: String, CodingKey {
        case id, title, status, priority, phase, due, tags, body
        case createdAt = "created_at"
    }

    public init(
        id: String, title: String, status: String = "pending", priority: String? = nil,
        phase: String? = nil, due: String? = nil, createdAt: String? = nil,
        tags: [String]? = nil, body: String? = nil
    ) {
        self.id = id
        self.title = title
        self.status = status
        self.priority = priority
        self.phase = phase
        self.due = due
        self.createdAt = createdAt
        self.tags = tags
        self.body = body
    }

    /// Title without Obsidian `[[wiki link]]` brackets.
    public var display: String {
        title.replacingOccurrences(of: "[[", with: "").replacingOccurrences(of: "]]", with: "")
    }

    public var isOpen: Bool { !Vocab.terminalStatuses.contains(status) }

    public var priorityRank: Int { Vocab.priorityRank(priority) }
}

/// A `.taskmd.yaml` phase. A phase without an id is keyed by its name.
public struct KPhase: Decodable, Hashable, Sendable {
    public let id: String?
    public let name: String

    public init(id: String?, name: String) {
        self.id = id
        self.name = name
    }

    public var key: String { id ?? name }
}

/// The part of `karamd config --json` the app uses.
public struct KConfig: Decodable, Hashable, Sendable {
    public let today: [String]
    public let phases: [KPhase]

    public init(today: [String], phases: [KPhase]) {
        self.today = today
        self.phases = phases
    }

    public static let empty = KConfig(today: [], phases: [])

    public func phaseName(_ key: String?) -> String? {
        phases.first { $0.key == key }?.name
    }
}

/// `karamd terminal-argv --json`.
public struct Launch: Decodable, Hashable, Sendable {
    public let argv: [String]
    public let cwd: String
}

public struct TaskSection: Hashable, Identifiable, Sendable {
    public let title: String
    /// Phase key the section stands for; nil for "Due", "" for "No phase".
    public var phase: String?
    public let tasks: [KTask]
    public var id: String { title }

    public init(title: String, phase: String? = nil, tasks: [KTask]) {
        self.title = title
        self.phase = phase
        self.tasks = tasks
    }
}

public struct Board: Hashable, Sendable {
    /// Today phases + due, for the menu bar count.
    public var focus: [KTask]
    public var sections: [TaskSection]
    public var overdue: Set<String>
    public var open: Int
    public var stale: Int
    public var error: String?

    public static let empty = Board(focus: [], sections: [], overdue: [], open: 0, stale: 0, error: nil)

    public static func failed(_ message: String) -> Board {
        var b = Board.empty
        b.error = message
        return b
    }
}

public struct CLIError: LocalizedError, Equatable {
    public let message: String

    public init(_ message: String) {
        self.message = message
    }

    public var errorDescription: String? { message }
}
