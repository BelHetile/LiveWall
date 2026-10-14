import Foundation
import Testing
import LiveWallCore
import LiveWallTools
@testable import LiveWallMedia

private let e2eRepoRoot = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent()
    .deletingLastPathComponent()
    .deletingLastPathComponent()
private let e2eFixtures = e2eRepoRoot.appendingPathComponent("Tests/Fixtures", isDirectory: true)

private struct TranscodeTimeout: Error {}

private func e2eWorkDirectory(_ label: String) -> URL {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("lw-e2e-\(label)-\(UUID().uuidString)", isDirectory: true)
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

private func e2eWaitForTerminal(
    _ coordinator: OptimizationCoordinator,
    _ jobID: UUID,
    timeout: TimeInterval = 120
) async -> OptimizationJobStatus? {
    let deadline = Date().addingTimeInterval(timeout)
    while Date() < deadline {
        if let status = await coordinator.status(jobID), status.state != .queued, status.state != .running {
            return status
        }
        try? await Task.sleep(for: .milliseconds(50))
    }
    return await coordinator.status(jobID)
}

@Suite(.enabled(if: ProcessInfo.processInfo.environment["LW_E2E"] == "1"))
struct TranscodeE2ETests {
    @Test func av1FixtureTranscodesToHEVC() async throws {
        let work = e2eWorkDirectory("av1")
        let input = e2eFixtures.appendingPathComponent("clip-av1-1080p.mp4")
        let output = work.appendingPathComponent("out.mov")
        let service = TranscodeService()
        let probe = MediaProbeService()
        let source = try await probe.readMetadata(input)
        let parameters = TranscodeParameters(maxHeight: 720, fpsLimit: 30)

        let result = try await withThrowingTaskGroup(of: TranscodeResult.self) { group in
            group.addTask {
                try await service.transcode(
                    input: input,
                    output: output,
                    source: source,
                    parameters: parameters
                )
            }
            group.addTask {
                try await Task.sleep(for: .seconds(120))
                await service.cancelActive()
                throw TranscodeTimeout()
            }
            guard let first = try await group.next() else {
                throw TranscodeTimeout()
            }
            group.cancelAll()
            return first
        }

        #expect(result.outputURL == output)
        #expect(result.videoCodec == "hevc")
        #expect(FileManager.default.fileExists(atPath: output.path))
        let attributes = try FileManager.default.attributesOfItem(atPath: output.path)
        let size = (attributes[.size] as? NSNumber)?.intValue ?? 0
        #expect(size > 1000)

        let reprobed = try await probe.readMetadata(output)
        #expect(reprobed.video?.codec == "hevc")
        #expect((reprobed.video?.height ?? 0) <= 720)
        #expect((reprobed.video?.height ?? 1) % 2 == 0)
        #expect((reprobed.video?.width ?? 1) % 2 == 0)
        #expect((reprobed.video?.frameRate ?? 999) <= 30.01)
    }

    @Test func hevcFixtureIsAlreadyLightEnough() async throws {
        let work = e2eWorkDirectory("hevc")
        let input = e2eFixtures.appendingPathComponent("clip-hevc-1080p30.mp4")
        let coordinator = OptimizationCoordinator(workDirectory: work)

        let id = await coordinator.enqueue(url: input, itemID: "fixture", maxHeight: 1080)
        let status = try #require(await e2eWaitForTerminal(coordinator, id))

        #expect(status.state == .done)
        guard case let .alreadyLightEnough(url) = status.outcome else {
            Issue.record("expected alreadyLightEnough, got \(String(describing: status.outcome))")
            return
        }
        #expect(url == input)
        #expect(((try? FileManager.default.contentsOfDirectory(atPath: work.path)) ?? []).isEmpty)
    }
}
