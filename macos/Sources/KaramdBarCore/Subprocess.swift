import Foundation

public struct ProcessResult: Sendable {
    public let status: Int32
    public let stdout: Data
    public let stderr: Data
    public let timedOut: Bool
}

public enum Subprocess {
    /// Runs on a GCD thread, not the Swift concurrency pool, because the waits block.
    /// SIGTERM after `timeout`; output a leftover grandchild still writes after exit is dropped.
    public static func run(
        _ executable: String, _ args: [String], environment: [String: String]? = nil, directory: URL? = nil,
        timeout: TimeInterval
    ) async throws -> ProcessResult {
        try await withCheckedThrowingContinuation { cont in
            DispatchQueue.global().async {
                cont.resume(
                    with: Result {
                        try runBlocking(executable, args, environment, directory, timeout)
                    })
            }
        }
    }

    private final class State: @unchecked Sendable {
        private let lock = NSLock()
        private var out = Data()
        private var err = Data()
        private var killed = false

        func append(_ d: Data, stderr: Bool) { lock.withLock { stderr ? err.append(d) : out.append(d) } }
        func markKilled() { lock.withLock { killed = true } }
        var snapshot: (Data, Data, Bool) { lock.withLock { (out, err, killed) } }
    }

    private static func runBlocking(
        _ executable: String, _ args: [String], _ environment: [String: String]?, _ directory: URL?,
        _ timeout: TimeInterval
    ) throws -> ProcessResult {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: executable)
        p.arguments = args
        if let environment { p.environment = environment }
        if let directory { p.currentDirectoryURL = directory }
        let state = State()
        let eof = DispatchGroup()
        for (pipe, isErr) in [(Pipe(), false), (Pipe(), true)] {
            if isErr { p.standardError = pipe } else { p.standardOutput = pipe }
            eof.enter()
            pipe.fileHandleForReading.readabilityHandler = { h in
                let d = h.availableData
                if d.isEmpty {
                    h.readabilityHandler = nil
                    eof.leave()
                } else {
                    state.append(d, stderr: isErr)
                }
            }
        }
        try p.run()

        let pid = p.processIdentifier
        let killer = DispatchWorkItem {
            state.markKilled()
            kill(pid, SIGTERM)
        }
        DispatchQueue.global().asyncAfter(deadline: .now() + timeout, execute: killer)
        p.waitUntilExit()
        killer.cancel()
        _ = eof.wait(timeout: .now() + 1)

        let (out, err, killed) = state.snapshot
        return ProcessResult(status: p.terminationStatus, stdout: out, stderr: err, timedOut: killed)
    }
}
