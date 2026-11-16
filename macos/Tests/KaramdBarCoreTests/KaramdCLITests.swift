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
            fake.calls == ["config --json", "list open:true --json", "show 001 --json", "terminal-argv 001 --json"])
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
        _ = try await cli.create("Buy milk", phase: nil)
        #expect(
            fake.calls == [
                "status 1 blocked", "complete 1", "postpone 1", "complete 2", "reopen 3", "status 4 completed",
                "edit 1 --phase ", "create Buy milk --phase now --json", "create Buy milk --json",
            ])
    }

    @Test func editArgs() {
        let args = KaramdCLI.editArgs("1", [.init("title", "x"), .init("due", "")], tags: ["a", "b"])
        #expect(args == ["edit", "1", "--title", "x", "--due", "", "--tag", "a", "--tag", "b"])
        #expect(KaramdCLI.editArgs("1", [], tags: nil) == ["edit", "1"])
    }

    @Test func loadBuildsTheBoard() async throws {
        let (config, tasks, board) = try await FakeKaramd().cli.load(today: day("2026-01-15"))
        #expect(config.phases.count == 4)
        #expect(tasks.count == 5)
        #expect(board.error == nil)
        #expect(!board.sections.isEmpty)
    }

    @Test func loadReportsFailure() async throws {
        let (config, tasks, board) = try await FakeKaramd("echo 'no vault' >&2; exit 3").cli.load()
        #expect(config == .empty)
        #expect(tasks.isEmpty)
        #expect(board.error == "no vault")
    }

    @Test func exitWithoutStderr() async throws {
        await #expect(throws: CLIError("karamd exited 4")) {
            try await FakeKaramd("exit 4").cli.complete("1")
        }
    }

    @Test func missingBinary() async {
        let cli = KaramdCLI(binary: "/nonexistent/karamd", vault: URL(fileURLWithPath: "/v"))
        await #expect(throws: CLIError.self) { try await cli.complete("1") }
    }

    @Test func badJSON() async throws {
        await #expect(throws: DecodingError.self) {
            _ = try await FakeKaramd("echo nope; exit 0").cli.config()
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

    /// The real binary on the fixture vault, when CI or `make` points `KARAMD_BIN` at it.
    @Test(.enabled(if: ProcessInfo.processInfo.environment["KARAMD_BIN"] != nil))
    func realBinary() async throws {
        let bin = try #require(ProcessInfo.processInfo.environment["KARAMD_BIN"])
        let cli = KaramdCLI(binary: bin, vault: Fixture.vault)
        let (_, tasks, board) = await cli.load()
        #expect(board.error == nil)
        #expect(tasks.map(\.id) == ["001", "002", "003", "005", "006"])
        #expect(try await cli.show("001").body?.contains("Transfer") == true)
    }
}
