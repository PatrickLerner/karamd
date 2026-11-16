import Foundation
import Testing

@testable import KaramdBarCore

/// The golden files are real karamd output; these break when the CLI shape drifts from the models.
struct ContractTests {
    @Test func listDecodesOpenTasks() throws {
        let tasks = try Fixture.tasks
        #expect(tasks.map(\.id) == ["001", "002", "003", "005", "006"])
        let rent = try #require(tasks.first)
        #expect(rent.title == "Pay [[rent]]")
        #expect(rent.priority == "high")
        #expect(rent.phase == "now")
        #expect(rent.due == "2026-01-10")
        #expect(rent.createdAt == "2025-11-01")
        #expect(rent.tags == ["home", "money"])
        #expect(rent.body == nil)
    }

    @Test func showIncludesBody() throws {
        let t: KTask = try Fixture.cli("show")
        #expect(t.body == "# Pay rent\n\nTransfer before the 10th.")
    }

    @Test func createReturnsTheNewTask() throws {
        let t: KTask = try Fixture.cli("create")
        #expect(t.id == "007")
        #expect(t.phase == "now")
        #expect(t.priority == "medium")
        #expect(t.createdAt == "2026-01-15")
    }

    @Test func configHasPhasesAndToday() throws {
        let c = try Fixture.config
        #expect(c.phases.map(\.key) == ["ongoing", "now", "next", "later"])
        #expect(c.today == ["ongoing", "now"])
    }

    @Test func terminalArgvHasArgvAndCwd() throws {
        let l: Launch = try Fixture.cli("terminal-argv")
        #expect(l.argv.first == "claude")
        #expect(l.cwd == "<vault>")
    }
}
