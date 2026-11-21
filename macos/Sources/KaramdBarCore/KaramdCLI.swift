import Foundation

/// All reads and writes go through the karamd binary.
public struct KaramdCLI: Sendable {
    public let binary: String
    public let vault: URL
    public let timeout: TimeInterval

    public init(binary: String, vault: URL, timeout: TimeInterval = 60) {
        self.binary = binary
        self.vault = vault
        self.timeout = timeout
    }

    /// Options go in `--flag=value` form and positionals after `--`, so a value starting with `-` stays a value.
    public static func argv(_ command: String, vault: URL, options: [String], positional: [String]) -> [String] {
        [command, "--vault=\(vault.path)"] + options + ["--"] + positional
    }

    @discardableResult
    public func run(_ command: String, _ options: [String] = [], _ positional: [String] = []) async throws -> Data {
        let args = Self.argv(command, vault: vault, options: options, positional: positional)
        let r: ProcessResult
        do { r = try await Subprocess.run(binary, args, timeout: timeout) } catch {
            throw CLIError("cannot run \(binary): \(error.localizedDescription)")
        }
        if r.timedOut { throw CLIError("karamd \(command) timed out after \(Int(timeout)) s") }
        guard r.status == 0 else {
            // validate prints its report to stdout.
            let msg = [r.stderr, r.stdout]
                .map { String(decoding: $0, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }.joined(separator: "\n")
            throw CLIError(msg.isEmpty ? "karamd exited \(r.status)" : msg)
        }
        return r.stdout
    }

    public func decode<T: Decodable>(_ command: String, _ options: [String] = [], _ positional: [String] = [])
        async throws -> T
    {
        try JSONDecoder().decode(T.self, from: try await run(command, options + ["--json"], positional))
    }

    public func config() async throws -> KConfig { try await decode("config") }
    public func openTasks() async throws -> [KTask] { try await decode("list", [], ["open:true"]) }
    public func show(_ id: String) async throws -> KTask { try await decode("show", [], [id]) }
    public func launch(_ id: String) async throws -> Launch { try await decode("terminal-argv", [], [id]) }

    public func setStatus(_ id: String, _ status: String) async throws { try await run("status", [], [id, status]) }
    public func complete(_ id: String) async throws { try await run("complete", [], [id]) }
    public func postpone(_ id: String) async throws { try await run("postpone", [], [id]) }
    public func reopen(_ id: String) async throws { try await run("reopen", [], [id]) }

    /// Renumbers duplicate ids. Any defect left fails the call with the report.
    public func validateFix() async throws { try await run("validate", ["--fix"]) }

    /// The error tells the user to run `validate`.
    public static func suggestsValidate(_ message: String) -> Bool { message.contains("validate") }

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
        let options = Self.editOptions(fields, tags: tags)
        guard !options.isEmpty else { return }
        try await run("edit", options, [id])
    }

    public func edit(_ id: String, _ flag: String, _ value: String) async throws {
        try await edit(id, [TaskEdit.Field(flag, value)])
    }

    public func create(_ title: String, phase: String?) async throws -> KTask {
        try await decode("create", phase.map { ["--phase=\($0)"] } ?? [], [title])
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

    public static func editOptions(_ fields: [TaskEdit.Field], tags: [String]?) -> [String] {
        fields.map { "--\($0.flag)=\($0.value)" } + (tags ?? []).map { "--tag=\($0)" }
    }

    /// Terminal args that run the agent in a login shell (an app started by launchd lacks the user's PATH),
    /// without Claude session markers from our own environment.
    public static func terminalArgs(_ l: Launch) -> [String] {
        ["--working-directory", l.cwd, "-e", "/bin/zsh", "-lc", #"unset -m "CLAUDE*"; exec "$@""#, "karamd"] + l.argv
    }

    /// Args for `alacritty msg`, which opens the window in the running instance behind `socket`.
    public static func createWindowArgs(socket: String, _ terminalArgs: [String]) -> [String] {
        ["msg", "--socket", socket, "create-window"] + terminalArgs
    }

    /// The environment without Claude session markers inherited from a parent session.
    public static func cleanEnv(_ env: [String: String]) -> [String: String] {
        env.filter { !$0.key.hasPrefix("CLAUDE") }
    }
}
