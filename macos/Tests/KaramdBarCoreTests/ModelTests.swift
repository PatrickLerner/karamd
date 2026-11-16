import Foundation
import Testing

@testable import KaramdBarCore

struct ModelTests {
    @Test func displayStripsWikiLinks() {
        #expect(KTask(id: "1", title: "Pay [[rent]] to [[Bob]]").display == "Pay rent to Bob")
    }

    @Test(arguments: [("pending", true), ("blocked", true), ("completed", false), ("cancelled", false)])
    func isOpen(status: String, open: Bool) {
        #expect(KTask(id: "1", title: "", status: status).isOpen == open)
    }

    @Test func phaseKeyFallsBackToName() {
        #expect(KPhase(id: "now", name: "Now").key == "now")
        #expect(KPhase(id: nil, name: "Now").key == "Now")
    }

    @Test func phaseName() throws {
        let c = try Fixture.config
        #expect(c.phaseName("next") == "Next")
        #expect(c.phaseName("nope") == nil)
        #expect(c.phaseName(nil) == nil)
        #expect(KConfig.empty.phases.isEmpty)
    }

    @Test func cliErrorDescribesItself() {
        #expect(CLIError("x").errorDescription == "x")
    }

    @Test(arguments: [
        ("critical", 0, "exclamationmark.3"), ("high", 1, "exclamationmark.2"), ("medium", 2, "minus"),
        ("low", 3, "arrow.down"), ("bogus", 2, "minus"),
    ])
    func priority(name: String, rank: Int, icon: String) {
        #expect(Vocab.priorityRank(name) == rank)
        #expect(Vocab.priorityIcon(name) == icon)
        #expect(Vocab.isUrgent(name) == (rank < 2))
        #expect(KTask(id: "1", title: "", priority: name).priorityRank == rank)
    }

    @Test func priorityIconFallback() {
        #expect(Vocab.priorityIcon(nil, fallback: "circle") == "circle")
        #expect(Vocab.priorityRank(nil) == 2)
    }

    @Test func everyStatusHasItsOwnIconAndName() {
        let icons = Vocab.statuses.map(Vocab.statusIcon)
        #expect(Set(icons).count == Vocab.statuses.count)
        #expect(Vocab.statusIcon("bogus") == "circle")
        #expect(
            Vocab.statuses.map(Vocab.statusName) == [
                "To Do", "In Progress", "In Review", "Blocked", "Completed", "Cancelled",
            ])
    }
}

struct DayTests {
    let now = day("2026-01-15")

    @Test func roundTrips() {
        #expect(Day.string(now, calendar: utc) == "2026-01-15")
        #expect(Day.date("2026-13-40", calendar: utc) == nil)
        #expect(Day.string(Day.adding(-30, to: now, calendar: utc), calendar: utc) == "2025-12-16")
    }

    @Test(arguments: [
        ("2026-01-15", "Today"), ("2026-01-16", "Tomorrow"), ("2026-01-14", "Yesterday"),
        ("2026-01-17", "Saturday"), ("2026-01-21", "Wednesday"), ("2026-01-22", "Jan 22"),
        ("2026-01-01", "Jan 1"), ("2027-03-01", "Mar 1, 2027"), ("garbage", "garbage"),
    ])
    func relative(input: String, want: String) {
        #expect(Day.relative(input, now: now, calendar: utc, locale: Locale(identifier: "en_US")) == want)
    }

    @Test func quickPicks() {
        let picks = Day.quickPicks(now: now, calendar: utc)
        #expect(picks.map(\.label) == ["Today", "Tomorrow", "Next Monday", "In a Week"])
        #expect(
            picks.map { Day.string($0.date, calendar: utc) } == [
                "2026-01-15", "2026-01-16", "2026-01-19", "2026-01-22",
            ])
    }

    @Test(arguments: [("2026-01-18", "2026-01-19"), ("2026-01-19", "2026-01-26"), ("2026-01-17", "2026-01-19")])
    func nextMonday(from: String, want: String) {
        #expect(Day.string(Day.quickPicks(now: day(from), calendar: utc)[2].date, calendar: utc) == want)
    }

    @Test func dstDayStillCountsAsOne() throws {
        var berlin = Calendar(identifier: .gregorian)
        berlin.timeZone = try #require(TimeZone(identifier: "Europe/Berlin"))
        let now = try #require(Day.date("2026-03-28", calendar: berlin))
        #expect(Day.relative("2026-03-29", now: now, calendar: berlin) == "Tomorrow")
        #expect(Day.relative("2026-03-30", now: now, calendar: berlin, locale: Locale(identifier: "en_US")) == "Monday")
    }
}

struct TaskEditTests {
    let task = KTask(id: "1", title: "Old", tags: ["a", "b"], body: "text")

    @Test func noChangeIsEmpty() {
        let e = TaskEdit.plan(from: task, title: "Old", tags: "a, b", body: "text")
        #expect(e == TaskEdit(fields: [], tags: nil, clearsAllTags: false))
        #expect(!TaskEdit.isDirty(task, title: "Old", tags: " a ,b, ", body: "text"))
    }

    @Test func titleBodyAndTags() {
        let e = TaskEdit.plan(from: task, title: "New", tags: "c", body: "")
        #expect(e.fields == [.init("title", "New"), .init("body", "")])
        #expect(e.tags == ["c"])
        #expect(!e.clearsAllTags)
        #expect(TaskEdit.isDirty(task, title: "New", tags: "a, b", body: "text"))
        #expect(TaskEdit.isDirty(task, title: "Old", tags: "a", body: "text"))
        #expect(TaskEdit.isDirty(task, title: "Old", tags: "a, b", body: ""))
    }

    @Test func clearingAllTagsIsFlagged() {
        let e = TaskEdit.plan(from: task, title: "Old", tags: " , ", body: "text")
        #expect(e.tags == nil)
        #expect(e.clearsAllTags)
    }

    @Test func taskWithoutTagsOrBody() {
        let bare = KTask(id: "1", title: "x")
        #expect(!TaskEdit.isDirty(bare, title: "x", tags: "", body: ""))
        #expect(
            TaskEdit.plan(from: bare, title: "x", tags: "", body: "")
                == .init(fields: [], tags: nil, clearsAllTags: false))
        #expect(TaskEdit.formatTags(nil) == "")
        #expect(TaskEdit.formatTags(["a", "b"]) == "a, b")
    }
}

struct CaptureRequestTests {
    @Test func promptNamesVaultAndDate() {
        let p = CaptureRequest.prompt(
            "buy milk", vault: URL(fileURLWithPath: "/v"), now: day("2026-01-15"), calendar: utc)
        #expect(p.contains("new task in /v. Today is 2026-01-15 (Thursday)."))
        #expect(p.contains("Request: buy milk"))
        #expect(p.hasPrefix("Use the filing-tasks skill"))
        let custom = CaptureRequest.prompt("x", vault: URL(fileURLWithPath: "/v"), skill: "file-task")
        #expect(custom.hasPrefix("Use the file-task skill"))
    }

    @Test func filed() {
        let o = CaptureRequest.parse("thinking\nFILED 042 Buy milk\n", exitCode: 0)
        #expect(o == .init(ok: true, message: "042 Buy milk", id: "042"))
    }

    @Test func failed() {
        let o = CaptureRequest.parse("FAILED no vault\nbye", exitCode: 1)
        #expect(o == .init(ok: false, message: "no vault", id: nil))
    }

    @Test func fallsBackToLastLineThenExitCode() {
        #expect(CaptureRequest.parse("a\n  oops  \n", exitCode: 2).message == "oops")
        #expect(CaptureRequest.parse("", exitCode: 2).message == "no output (exit 2)")
    }

    @Test func commandReadsPromptFromEnv() {
        #expect(CaptureRequest.command.contains(#""$KARAMD_PROMPT""#))
    }
}
