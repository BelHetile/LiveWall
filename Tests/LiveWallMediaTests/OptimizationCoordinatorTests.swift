import Foundation
import Testing
import LiveWallCore
import LiveWallTools
@testable import LiveWallMedia

private func makeTempDirectory(_ label: String) -> URL {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("lwopt-\(label)-\(UUID().uuidString)", isDirectory: true)
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

private func writeExecutable(_ script: String, named name: String, in directory: URL) {
    let url = directory.appendingPathComponent(name)
    try? script.write(to: url, atomically: true, encoding: .utf8)
    try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
}

private func video(
    height: Int = 1080,
    width: Int = 1920,
    frameRate: Double = 30
) -> VideoMetadata {
    return VideoMetadata(
        codec: "av1",
        profile: nil,
        pixelFormat: "yuv420p",
        bitDepth: 8,
        colorSpace: nil,
        colorPrimaries: nil,
        colorTransfer: nil,
        hdrFormat: nil,
        width: width,
        height: height,
        frameRate: frameRate,
        frameCount: nil
    )
}

private func metadata(height: Int = 1080, width: Int = 1920, frameRate: Double = 30) -> MediaMetadata {
    return MediaMetadata(
        container: "mov,mp4",
        duration: 2,
        bitRate: nil,
        video: video(height: height, width: width, frameRate: frameRate),
        audio: nil
    )
}

private func transcodeProbe(height: Int = 1080, width: Int = 1920, frameRate: Double = 30) -> MediaProbeResult {
    return MediaProbeResult(
        kind: .video,
        verdict: .transcodeToHEVC(warning: "av1"),
        metadata: metadata(height: height, width: width, frameRate: frameRate)
    )
}

private func nativeProbe(height: Int = 1080, frameRate: Double = 30) -> MediaProbeResult {
    return MediaProbeResult(
        kind: .video,
        verdict: .native,
        metadata: metadata(height: height, frameRate: frameRate)
    )
}

private func makeCoordinator(root: URL, work: URL) -> OptimizationCoordinator {
    return OptimizationCoordinator(
        resolver: ToolLocator(searchDirectories: [root]),
        workDirectory: work,
        av1HardwareSupport: { false },
        hevcAvailable: { true },
        fpsLimitProvider: { 30 }
    )
}

private func orderingStub(root: URL, sleep: String = "0.3") -> String {
    let log = root.appendingPathComponent("ordering.log").path
    return """
    #!/bin/sh
    printf 'start\\n' >> "\(log)"
    sleep \(sleep)
    last=""
    for a in "$@"; do last="$a"; done
    : > "$last"
    printf 'end\\n' >> "\(log)"
    exit 0
    """
}

private func contentStub(_ content: String) -> String {
    return """
    #!/bin/sh
    last=""
    for a in "$@"; do last="$a"; done
    printf '%s' '\(content)' > "$last"
    exit 0
    """
}

private func slowStub(root: URL) -> String {
    let marker = root.appendingPathComponent("started").path
    return """
    #!/bin/sh
    : > "\(marker)"
    exec sleep 30
    """
}

private func orderingLog(root: URL) -> [String] {
    guard let contents = try? String(contentsOf: root.appendingPathComponent("ordering.log"), encoding: .utf8) else {
        return []
    }
    return contents.split(separator: "\n").map(String.init)
}

private func waitForFile(_ url: URL, timeout: TimeInterval = 5) async -> Bool {
    let deadline = Date().addingTimeInterval(timeout)
    while Date() < deadline {
        if FileManager.default.fileExists(atPath: url.path) {
            return true
        }
        try? await Task.sleep(for: .milliseconds(10))
    }
    return FileManager.default.fileExists(atPath: url.path)
}

private func waitForTerminal(
    _ coordinator: OptimizationCoordinator,
    _ jobID: UUID,
    timeout: TimeInterval = 8
) async -> OptimizationJobStatus? {
    let deadline = Date().addingTimeInterval(timeout)
    while Date() < deadline {
        if let status = await coordinator.status(jobID), status.state != .queued, status.state != .running {
            return status
        }
        try? await Task.sleep(for: .milliseconds(20))
    }
    return await coordinator.status(jobID)
}

@Suite struct OptimizationDecisionTests {
    @Test func decisionMatrix() {
        #expect(OptimizationCoordinator.decide(probeResult: nativeProbe(height: 1080), maxHeight: 1080, fpsLimit: 30) == .alreadyLightEnough)
        #expect(OptimizationCoordinator.decide(probeResult: nativeProbe(height: 2160), maxHeight: 1080, fpsLimit: 30) == .transcode)
        #expect(OptimizationCoordinator.decide(probeResult: nativeProbe(height: 720, frameRate: 60), maxHeight: 1080, fpsLimit: 30) == .transcode)
        #expect(OptimizationCoordinator.decide(probeResult: transcodeProbe(), maxHeight: 1080, fpsLimit: 30) == .transcode)
        #expect(OptimizationCoordinator.decide(
            probeResult: MediaProbeResult(kind: nil, verdict: .rejected(reason: "bad"), metadata: nil),
            maxHeight: 1080,
            fpsLimit: 30
        ) == .rejected(reason: "bad"))
    }

    @Test func defaultFpsLimitIsClampedOrDefaulted() {
        let value = OptimizationCoordinator.defaultFpsLimit()
        #expect(value >= 15 && value <= 120)
    }
}

@Suite struct OptimizationCoordinatorTests {
    @Test func jobsRunStrictlySerially() async throws {
        let root = makeTempDirectory("serial")
        let work = makeTempDirectory("serial-work")
        writeExecutable(orderingStub(root: root), named: "ffmpeg", in: root)
        let coordinator = makeCoordinator(root: root, work: work)
        let source = root.appendingPathComponent("source.mp4")
        try Data("v".utf8).write(to: source)

        let first = await coordinator.enqueue(url: source, itemID: "a", maxHeight: 720, presetProbe: transcodeProbe())
        let second = await coordinator.enqueue(url: source, itemID: "b", maxHeight: 720, presetProbe: transcodeProbe())

        let firstStatus = await waitForTerminal(coordinator, first)
        let secondStatus = await waitForTerminal(coordinator, second)
        #expect(firstStatus?.state == .done)
        #expect(secondStatus?.state == .done)
        #expect(orderingLog(root: root) == ["start", "end", "start", "end"])
    }

    @Test func nativeWithinTargetSkipsTranscode() async throws {
        let root = makeTempDirectory("skip")
        let work = makeTempDirectory("skip-work")
        let coordinator = makeCoordinator(root: root, work: work)
        let source = root.appendingPathComponent("source.mp4")
        try Data("v".utf8).write(to: source)

        let id = await coordinator.enqueue(
            url: source,
            itemID: "a",
            maxHeight: 1080,
            presetProbe: nativeProbe(height: 1080)
        )
        let status = await waitForTerminal(coordinator, id)

        #expect(status?.state == .done)
        #expect(status?.outcome == .alreadyLightEnough(url: source))
        #expect(((try? FileManager.default.contentsOfDirectory(atPath: work.path)) ?? []).isEmpty)
    }

    @Test func oversizedNativeTranscodes() async throws {
        let root = makeTempDirectory("oversized")
        let work = makeTempDirectory("oversized-work")
        writeExecutable(contentStub("transcoded"), named: "ffmpeg", in: root)
        let coordinator = makeCoordinator(root: root, work: work)
        let source = root.appendingPathComponent("source.mp4")
        try Data("v".utf8).write(to: source)

        let id = await coordinator.enqueue(
            url: source,
            itemID: "a",
            maxHeight: 720,
            presetProbe: nativeProbe(height: 2160)
        )
        let status = await waitForTerminal(coordinator, id)

        #expect(status?.state == .done)
        guard case .transcoded = status?.outcome else {
            Issue.record("expected transcoded, got \(String(describing: status?.outcome))")
            return
        }
    }

    @Test func rejectedJobIsTypedRejection() async throws {
        let root = makeTempDirectory("rejected")
        let work = makeTempDirectory("rejected-work")
        let coordinator = makeCoordinator(root: root, work: work)
        let source = root.appendingPathComponent("source.mp4")
        try Data("v".utf8).write(to: source)

        let id = await coordinator.enqueue(
            url: source,
            itemID: "a",
            maxHeight: 720,
            presetProbe: MediaProbeResult(kind: nil, verdict: .rejected(reason: "nope"), metadata: nil)
        )
        let status = await waitForTerminal(coordinator, id)

        #expect(status?.state == .failed)
        #expect(status?.outcome == .rejected(reason: "nope"))
        #expect(status?.error?.contains("nope") == true)
    }

    @Test func defaultReturnsWorkDirectoryOutputAndLeavesSource() async throws {
        let root = makeTempDirectory("default")
        let work = makeTempDirectory("default-work")
        writeExecutable(contentStub("transcoded"), named: "ffmpeg", in: root)
        let coordinator = makeCoordinator(root: root, work: work)
        let source = root.appendingPathComponent("source.mp4")
        try Data("original".utf8).write(to: source)

        let id = await coordinator.enqueue(url: source, itemID: "a", maxHeight: 720, presetProbe: transcodeProbe())
        let status = await waitForTerminal(coordinator, id)

        #expect(status?.state == .done)
        let resultURL = try #require(status?.outcome?.url)
        #expect(resultURL.pathExtension == "mov")
        #expect(resultURL.deletingLastPathComponent().lastPathComponent == id.uuidString)
        #expect(resultURL != source)
        #expect(try String(contentsOf: source, encoding: .utf8) == "original")
    }

    @Test func replaceInPlaceReplacesSourceAndWipesWork() async throws {
        let root = makeTempDirectory("replace")
        let work = makeTempDirectory("replace-work")
        writeExecutable(contentStub("transcoded"), named: "ffmpeg", in: root)
        let coordinator = makeCoordinator(root: root, work: work)
        let source = root.appendingPathComponent("source.mp4")
        try Data("original".utf8).write(to: source)

        let id = await coordinator.enqueue(
            url: source,
            itemID: "a",
            maxHeight: 720,
            replaceInPlace: true,
            presetProbe: transcodeProbe()
        )
        let status = await waitForTerminal(coordinator, id)

        #expect(status?.state == .done)
        #expect(status?.outcome?.url == source)
        #expect(try String(contentsOf: source, encoding: .utf8) == "transcoded")
        #expect(((try? FileManager.default.contentsOfDirectory(atPath: work.path)) ?? []).isEmpty)
    }

    @Test func cancelDeletesPartialAndLeavesSource() async throws {
        let root = makeTempDirectory("cancel")
        let work = makeTempDirectory("cancel-work")
        writeExecutable(slowStub(root: root), named: "ffmpeg", in: root)
        let coordinator = makeCoordinator(root: root, work: work)
        let source = root.appendingPathComponent("source.mp4")
        try Data("original".utf8).write(to: source)

        let id = await coordinator.enqueue(url: source, itemID: "a", maxHeight: 720, presetProbe: transcodeProbe())
        #expect(await waitForFile(root.appendingPathComponent("started")))

        await coordinator.cancel(id)
        let status = await waitForTerminal(coordinator, id)

        #expect(status?.state == .canceled)
        #expect(status?.outcome == nil)
        #expect(try String(contentsOf: source, encoding: .utf8) == "original")
        #expect(((try? FileManager.default.contentsOfDirectory(atPath: work.path)) ?? []).isEmpty)
    }

    @Test func cancelAllCancelsRunningAndQueued() async throws {
        let root = makeTempDirectory("cancelall")
        let work = makeTempDirectory("cancelall-work")
        writeExecutable(slowStub(root: root), named: "ffmpeg", in: root)
        let coordinator = makeCoordinator(root: root, work: work)
        let source = root.appendingPathComponent("source.mp4")
        try Data("v".utf8).write(to: source)

        let first = await coordinator.enqueue(url: source, itemID: "a", maxHeight: 720, presetProbe: transcodeProbe())
        let second = await coordinator.enqueue(url: source, itemID: "b", maxHeight: 720, presetProbe: transcodeProbe())
        #expect(await waitForFile(root.appendingPathComponent("started")))

        await coordinator.cancelAll()

        #expect(await waitForTerminal(coordinator, first)?.state == .canceled)
        #expect(await waitForTerminal(coordinator, second)?.state == .canceled)
        #expect(((try? FileManager.default.contentsOfDirectory(atPath: work.path)) ?? []).isEmpty)
    }

    @Test func eventsReportLifecycle() async throws {
        let root = makeTempDirectory("events")
        let work = makeTempDirectory("events-work")
        writeExecutable(contentStub("transcoded"), named: "ffmpeg", in: root)
        let coordinator = makeCoordinator(root: root, work: work)
        let source = root.appendingPathComponent("source.mp4")
        try Data("v".utf8).write(to: source)

        let stream = await coordinator.events()
        let collector = Task { () -> [OptimizationEvent] in
            var events: [OptimizationEvent] = []
            for await event in stream {
                events.append(event)
                if case .finished = event {
                    break
                }
            }
            return events
        }

        let id = await coordinator.enqueue(url: source, itemID: "a", maxHeight: 720, presetProbe: transcodeProbe())
        _ = await waitForTerminal(coordinator, id)
        let events = await collector.value

        #expect(events.contains { if case .queued = $0 { return true } else { return false } })
        #expect(events.contains { if case .started = $0 { return true } else { return false } })
        #expect(events.contains { if case .finished = $0 { return true } else { return false } })
    }

    @Test func unknownJobHasNoStatus() async throws {
        let root = makeTempDirectory("unknown")
        let work = makeTempDirectory("unknown-work")
        let coordinator = makeCoordinator(root: root, work: work)
        #expect(await coordinator.status(UUID()) == nil)
    }
}
