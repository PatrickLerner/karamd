import Foundation
import Testing

@testable import KaramdBarCore

struct KaramdCLITests {
    @Test func reads() async throws {
        let fake = try FakeKaramd()
        let cli = fake.cli
        #expect(try await cli.config().today == ["ongoing", "now"])
        #expect(try await cli.openTasks().count == 5)
        #expect(try await cli.show("001").body != nil)
        #expect(try await cli.launch("001").argv.first == "claude")
        #expect(
            fake.calls == [
                ["config", "--json", "--"], ["list", "--json", "--", "open:true"], ["show", "--json", "--", "001"],
                ["terminal-argv", "--json", "--", "001"],
            ])
    }

    @Test func writes() async throws {
        let fake = try FakeKaramd()
        let cli = fake.cli
        try await cli.setStatus("1", "blocked")
        try await cli.complete("1")
        try await cli.postpone("1")
        try await cli.toggle(KTask(id: "2", title: ""))
        try await cli.toggle(KTask(id: "3", title: "", status: "completed"))
        try await cli.toggle(KTask(id: "4", title: "", status: "in-review"))
        try await cli.edit("1", "phase", "")
        try await cli.edit("1", [], tags: nil)
        #expect(try await cli.create("Buy milk", phase: "now").id == "007")
        _ = try await cli.create("-x", phase: nil)
        #expect(
            fake.calls == [
                ["status", "--", "1", "blocked"], ["complete", "--", "1"], ["postpone", "--", "1"],
                ["complete", "--", "2"], ["reopen", "--", "3"], ["status", "--", "4", "completed"],
                ["edit", "--phase=", "--", "1"], ["create", "--phase=now", "--json", "--", "Buy milk"],
                ["create", "--json", "--", "-x"],
            ])
    }

    @Test func editOptions() {
        let options = KaramdCLI.editOptions([.init("body", "- a"), .init("due", "")], tags: ["a", "-b"])
        #expect(options == ["--body=- a", "--due=", "--tag=a", "--tag=-b"])
        #expect(KaramdCLI.editOptions([], tags: nil).isEmpty)
    }

    @Test func loadBuildsTheBoard() async throws {
        let fake = try FakeKaramd()
        let (config, tasks, board) = await fake.cli.load(today: day("2026-01-15"))
        #expect(config.phases.count == 4)
        #expect(tasks.count == 5)
        #expect(board.error == nil)
        #expect(!board.sections.isEmpty)
    }

    @Test func loadReportsFailure() async throws {
        let fake = try FakeKaramd("echo 'no vault' >&2; exit 3")
        let (config, tasks, board) = await fake.cli.load()
        #expect(config == .empty)
        #expect(tasks.isEmpty)
        #expect(board.error == "no vault")
    }

    @Test func exitWithoutStderr() async throws {
        await #expect(throws: CLIError("karamd exited 4")) {
            let fake = try FakeKaramd("exit 4")
            try await fake.cli.complete("1")
        }
    }

    @Test func hangingCLITimesOut() async throws {
        await #expect(throws: CLIError("karamd complete timed out after 0 s")) {
            let fake = try FakeKaramd("sleep 5", timeout: 0.3)
            try await fake.cli.complete("1")
        }
    }

    /// More stderr than a pipe buffer holds must not block stdout.
    @Test func largeStderrDoesNotDeadlock() async throws {
        let fake = try FakeKaramd("head -c 200000 /dev/zero | tr '\\0' x >&2")
        #expect(try await fake.cli.config().phases.count == 4)
    }

    @Test func missingBinary() async {
        let cli = KaramdCLI(binary: "/nonexistent/karamd", vault: URL(fileURLWithPath: "/v"))
        await #expect(throws: CLIError.self) { try await cli.complete("1") }
    }

    @Test func badJSON() async throws {
        await #expect(throws: DecodingError.self) {
            let fake = try FakeKaramd("echo nope; exit 0")
            _ = try await fake.cli.config()
        }
    }

    @Test func terminalArgs() {
        let args = KaramdCLI.terminalArgs(Launch(argv: ["claude", "go"], cwd: "/v"))
        #expect(args.prefix(4) == ["--working-directory", "/v", "-e", "/bin/zsh"])
        #expect(args.suffix(3) == ["karamd", "claude", "go"])
    }

    @Test func cleanEnvDropsClaudeMarkers() {
        #expect(KaramdCLI.cleanEnv(["CLAUDECODE": "1", "CLAUDE_X": "1", "PATH": "/bin"]) == ["PATH": "/bin"])
    }

    static let realBin = ProcessInfo.processInfo.environment["KARAMD_BIN"]

    /// The real binary on a copy of the fixture vault, when CI or check.sh points `KARAMD_BIN` at it.
    @Test(.enabled(if: realBin != nil))
    func realBinary() async throws {
        let vault = try Scratch.dir().appending(path: "vault")
        try FileManager.default.copyItem(at: Fixture.vault, to: vault)
        let cli = KaramdCLI(binary: try #require(Self.realBin), vault: vault)

        let (_, tasks, board) = await cli.load()
        #expect(board.error == nil)
        #expect(tasks.map(\.id) == ["001", "002", "003", "005", "006"])

        // Values that look like flags must stay values.
        let created = try await cli.create("-x thing", phase: "now")
        #expect(created.title == "-x thing")
        try await cli.edit(created.id, [.init("body", "- a\n- b"), .init("due", "2026-02-01")], tags: ["-t"])
        let t = try await cli.show(created.id)
        #expect(t.body == "- a\n- b")
        #expect(t.due == "2026-02-01")
        #expect(t.tags == ["-t"])

        try await cli.toggle(t)
        #expect(try await cli.show(created.id).status == "completed")
        try await cli.toggle(try await cli.show(created.id))
        #expect(try await cli.show(created.id).status == "pending")
        try await cli.postpone(created.id)
        try await cli.setStatus(created.id, "blocked")
        #expect(try await cli.show(created.id).status == "blocked")
        #expect(try await cli.launch(created.id).cwd == vault.path)
    }
}
