import Foundation

/// All reads and writes go through the karamd binary.
public struct KaramdCLI: Sendable {
    public let binary: String
    public let vault: URL

    public init(binary: String, vault: URL) {
        self.binary = binary
        self.vault = vault
    }

    @discardableResult
    public func run(_ args: [String]) async throws -> Data {
        let binary = binary
        let args = args + ["--vault", vault.path]
        return try await Task.detached {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: binary)
            p.arguments = args
            let out = Pipe()
            let err = Pipe()
            p.standardOutput = out
            p.standardError = err
            do { try p.run() } catch {
                throw CLIError("cannot run \(binary): \(error.localizedDescription)")
            }
            let data = out.fileHandleForReading.readDataToEndOfFile()
            let errData = err.fileHandleForReading.readDataToEndOfFile()
            p.waitUntilExit()
            guard p.terminationStatus == 0 else {
                let msg = String(decoding: errData, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                throw CLIError(msg.isEmpty ? "karamd exited \(p.terminationStatus)" : msg)
            }
            return data
        }.value
    }

    public func decode<T: Decodable>(_ args: [String]) async throws -> T {
        try JSONDecoder().decode(T.self, from: try await run(args + ["--json"]))
    }

    public func config() async throws -> KConfig { try await decode(["config"]) }
    public func openTasks() async throws -> [KTask] { try await decode(["list", "open:true"]) }
    public func show(_ id: String) async throws -> KTask { try await decode(["show", id]) }
    public func launch(_ id: String) async throws -> Launch { try await decode(["terminal-argv", id]) }

    public func setStatus(_ id: String, _ status: String) async throws { try await run(["status", id, status]) }
    public func complete(_ id: String) async throws { try await run(["complete", id]) }
    public func postpone(_ id: String) async throws { try await run(["postpone", id]) }
    public func reopen(_ id: String) async throws { try await run(["reopen", id]) }

    /// Complete an open task, reopen a closed one. `complete` only moves to in-review under the
    /// pr-review workflow, so an in-review task is completed by status.
    public func toggle(_ t: KTask) async throws {
        if !t.isOpen {
            try await reopen(t.id)
        } else if t.status == "in-review" {
            try await setStatus(t.id, "completed")
        } else {
            try await complete(t.id)
        }
    }

    /// "" clears a field. Does nothing when there is nothing to change.
    public func edit(_ id: String, _ fields: [TaskEdit.Field], tags: [String]? = nil) async throws {
        let args = Self.editArgs(id, fields, tags: tags)
        guard args.count > 2 else { return }
        try await run(args)
    }

    public func edit(_ id: String, _ flag: String, _ value: String) async throws {
        try await edit(id, [TaskEdit.Field(flag, value)])
    }

    public func create(_ title: String, phase: String?) async throws -> KTask {
        try await decode(["create", title] + (phase.map { ["--phase", $0] } ?? []))
    }

    /// Config, open tasks and the Board; a failure becomes a Board with `error` set.
    public func load(today: Date = .now) async -> (KConfig, [KTask], Board) {
        do {
            async let cfg = config()
            async let list = openTasks()
            let (c, t) = try await (cfg, list)
            return (c, t, Board.build(tasks: t, config: c, today: today))
        } catch {
            return (.empty, [], .failed(error.localizedDescription))
        }
    }

    public static func editArgs(_ id: String, _ fields: [TaskEdit.Field], tags: [String]?) -> [String] {
        ["edit", id] + fields.flatMap { ["--\($0.flag)", $0.value] } + (tags ?? []).flatMap { ["--tag", $0] }
    }

    /// Terminal args that run the agent in a login shell (an app started by launchd lacks the user's PATH),
    /// without Claude session markers from our own environment.
    public static func terminalArgs(_ l: Launch) -> [String] {
        ["--working-directory", l.cwd, "-e", "/bin/zsh", "-lc", #"unset -m "CLAUDE*"; exec "$@""#, "karamd"] + l.argv
    }

    /// The environment without Claude session markers inherited from a parent session.
    public static func cleanEnv(_ env: [String: String]) -> [String: String] {
        env.filter { !$0.key.hasPrefix("CLAUDE") }
    }
}
