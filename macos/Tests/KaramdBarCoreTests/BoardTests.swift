import Foundation
import Testing

@testable import KaramdBarCore

struct BoardTests {
    let today = day("2026-01-15")

    func board() throws -> Board {
        try Board.build(tasks: Fixture.tasks, config: Fixture.config, today: today, calendar: utc)
    }

    @Test func todayPhasesThenDueThenNoPhase() throws {
        let b = try board()
        #expect(b.sections.map(\.title) == ["Ongoing", "Now", "Due", "No phase"])
        #expect(b.sections.map { $0.tasks.map(\.id) } == [["005"], ["001"], ["002"], ["006", "003"]])
        #expect(b.sections.map(\.phase) == ["ongoing", "now", nil, nil])
        #expect(b.sections.map(\.id) == b.sections.map(\.title))
        #expect(b.focus.map(\.id) == ["005", "001", "002"])
        #expect(b.overdue == ["001"])
        #expect(b.open == 5)
        #expect(b.stale == 1)
        #expect(b.error == nil)
        #expect(b.total == 5)
    }

    @Test func dropsClosedTasksAndEmptySections() {
        let tasks = [KTask(id: "1", title: "a", status: "completed", phase: "now")]
        let b = Board.build(tasks: tasks, config: KConfig(today: ["now"], phases: []), today: today, calendar: utc)
        #expect(b.sections.isEmpty)
        #expect(b.open == 0)
    }

    @Test func unknownTodayPhaseUsesItsKeyAsTitle() {
        let tasks = [KTask(id: "1", title: "a", phase: "x")]
        let b = Board.build(tasks: tasks, config: KConfig(today: ["x"], phases: []), today: today, calendar: utc)
        #expect(b.sections.map(\.title) == ["x"])
    }

    @Test func cappedKeepsOrderAndDropsTheRest() throws {
        let capped = try board().capped(2)
        #expect(capped.map(\.title) == ["Ongoing", "Now"])
        #expect(capped.map(\.phase) == ["ongoing", "now"])
        let partial = try board().capped(0)
        #expect(partial.isEmpty)
    }

    @Test func failedBoardCarriesTheMessage() {
        #expect(Board.failed("boom").error == "boom")
        #expect(Board.failed("boom").sections.isEmpty)
    }

    @Test func byPhasePutsUnknownAboveNext() throws {
        let s = try TaskSection.byPhase(tasks: Fixture.tasks, config: Fixture.config, overdue: [])
        #expect(s.map(\.title) == ["Ongoing", "Now", "No phase", "Next", "Later"])
        #expect(s[2].phase == "")
        #expect(s[2].tasks.map(\.id) == ["006", "003"])
        #expect(s[4].tasks.isEmpty)
    }

    @Test func byPhaseAppendsNoPhaseWithoutNext() {
        let config = KConfig(today: [], phases: [KPhase(id: nil, name: "Inbox")])
        let s = TaskSection.byPhase(tasks: [KTask(id: "1", title: "a")], config: config, overdue: [])
        #expect(s.map(\.title) == ["Inbox", "No phase"])
        #expect(s[0].phase == "Inbox")
    }

    @Test func byPhaseOmitsEmptyNoPhase() throws {
        let s = try TaskSection.byPhase(tasks: [], config: Fixture.config, overdue: [])
        #expect(s.map(\.title) == ["Ongoing", "Now", "Next", "Later"])
    }

    @Test func sortsOverdueThenPriorityThenDueThenId() {
        let tasks = [
            KTask(id: "4", title: "", priority: "low"),
            KTask(id: "3", title: "", due: "2026-02-01"),
            KTask(id: "2", title: "", due: "2026-01-20"),
            KTask(id: "1", title: ""),
            KTask(id: "5", title: "", priority: "critical"),
            KTask(id: "6", title: "", priority: "low"),
        ]
        let ids = KTask.sorted(tasks, overdue: ["6"]).map(\.id)
        #expect(ids == ["6", "5", "2", "3", "1", "4"])
    }

    @Test(arguments: [
        ("", true), ("RENT", true), ("00", true), ("mon", true), ("  home ", true), ("zzz", false),
    ])
    func matches(query: String, hit: Bool) throws {
        #expect(try Fixture.tasks[0].matches(query) == hit)
    }

    @Test func matchesWithoutTags() {
        #expect(!KTask(id: "1", title: "a").matches("b"))
    }
}
