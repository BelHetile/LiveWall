import Foundation
import Testing
import LiveWallCore
@testable import LiveWallTools

private func makeTempDir(_ label: String) -> URL {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("lwdl-\(label)-\(UUID().uuidString)", isDirectory: true)
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

private func writeExecutable(_ script: String, named name: String, in directory: URL) {
    let url = directory.appendingPathComponent(name)
    try? script.write(to: url, atomically: true, encoding: .utf8)
    try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
}

private func jobDirectory(incoming: URL, jobID: UUID) -> URL {
    return incoming.appendingPathComponent(jobID.uuidString, isDirectory: true)
}

private func makeService(root: URL, incoming: URL, hwAV1: Bool = false) -> DownloadService {
    return DownloadService(
        resolver: ToolLocator(searchDirectories: [root]),
        incomingDirectory: incoming,
        hardwareAV1: { hwAV1 }
    )
}

private func waitForTerminal(
    _ service: DownloadService,
    _ jobID: UUID,
    timeout: TimeInterval = 5
) async -> DownloadJobStatus? {
    let deadline = Date().addingTimeInterval(timeout)
    while Date() < deadline {
        if let status = await service.status(jobID), status.state != .running {
            return status
        }
        try? await Task.sleep(for: .milliseconds(20))
    }
    return await service.status(jobID)
}

private func waitForFile(_ url: URL, timeout: TimeInterval = 2) async -> Bool {
    let deadline = Date().addingTimeInterval(timeout)
    while Date() < deadline {
        if FileManager.default.fileExists(atPath: url.path) {
            return true
        }
        try? await Task.sleep(for: .milliseconds(10))
    }
    return FileManager.default.fileExists(atPath: url.path)
}

private let successBehavior = """
dir=$(dirname "$out")
mkdir -p "$dir"
: > "$dir/out.mp4"
printf 'LWPROG:100.0%%:0: 1.0MiB/s:%s\\n' "$fmt"
exit 0
"""

private let formatUnavailableBehavior = """
case "$fmt" in
  bv*) printf 'ERROR: Requested format is not available\\n' >&2; exit 1 ;;
esac
dir=$(dirname "$out")
mkdir -p "$dir"
: > "$dir/out.mp4"
exit 0
"""

private let hardFailureBehavior = """
printf 'boom\\n' >&2
exit 1
"""

private let cancelBehavior = """
dir=$(dirname "$out")
mkdir -p "$dir"
: > "$dir/started"
exec sleep 30
"""

private let slowBehavior = """
dir=$(dirname "$out")
mkdir -p "$dir"
: > "$dir/started"
sleep 0.5
: > "$dir/out.mp4"
exit 0
"""

private func ytDlpStub(root: URL, title: String, behavior: String) -> String {
    let log = root.appendingPathComponent("formats.txt").path
    return """
    #!/bin/sh
    fmt=""
    out=""
    while [ $# -gt 0 ]; do
      case "$1" in
        -f) fmt="$2"; shift 2 ;;
        -o) out="$2"; shift 2 ;;
        *) shift ;;
      esac
    done
    printf '%s\\n' "$fmt" >> "\(log)"
    printf 'LWTITLE:%s\\n' "\(title)"
    printf 'LWPROG: 20.0%%:5: 1.0MiB/s:%s\\n' "$fmt"
    printf 'LWPROG: 60.0%%:2: 1.0MiB/s:%s\\n' "$fmt"
    \(behavior)
    """
}

private func ffmpegStub(root: URL, succeed: Bool) -> String {
    let log = root.appendingPathComponent("ffmpeg-args.txt").path
    if succeed {
        return """
        #!/bin/sh
        printf '%s\\n' "$@" > "\(log)"
        last=""
        for a in "$@"; do last="$a"; done
        : > "$last"
        exit 0
        """
    }
    return """
    #!/bin/sh
    printf '%s\\n' "$@" > "\(log)"
    printf 'mux exploded\\n' >&2
    exit 1
    """
}

private func recordedFormats(in root: URL) -> [String] {
    guard let contents = try? String(contentsOf: root.appendingPathComponent("formats.txt"), encoding: .utf8) else {
        return []
    }
    return contents.split(separator: "\n").map(String.init)
}

@Suite struct DownloadServiceRoutingTests {
    @Test func splitRouteDownloadsBothStreamsAndMuxes() async throws {
        let root = makeTempDir("split")
        let incoming = makeTempDir("split-incoming")
        writeExecutable(ytDlpStub(root: root, title: "Stub Video", behavior: successBehavior), named: "yt-dlp", in: root)
        writeExecutable(ffmpegStub(root: root, succeed: true), named: "ffmpeg", in: root)
        let service = makeService(root: root, incoming: incoming)

        let jobID = try await service.start(url: URL(string: "https://example.com/v")!, maxHeight: 1080)
        let status = try #require(await waitForTerminal(service, jobID))

        #expect(status.state == .done)
        #expect(status.progress == 100)
        let path = try #require(status.path)
        #expect(path.lastPathComponent == "Stub Video.mp4")
        #expect(FileManager.default.fileExists(atPath: path.path))
        #expect(!FileManager.default.fileExists(atPath: jobDirectory(incoming: incoming, jobID: jobID).path))

        let formats = recordedFormats(in: root)
        #expect(formats.first == DownloadFormatSelector.videoExpression(maxHeight: 1080, hwAV1: false))
        #expect(formats.contains(DownloadFormatSelector.audioExpression()))

        let args = try String(contentsOf: root.appendingPathComponent("ffmpeg-args.txt"), encoding: .utf8)
            .split(separator: "\n").map(String.init)
        #expect(args.contains("-c"))
        #expect(args.contains("copy"))
        #expect(args.last?.hasSuffix("output.mp4") == true)
    }

    @Test func missingSplitFormatsFallBackToBestSingle() async throws {
        let root = makeTempDir("single")
        let incoming = makeTempDir("single-incoming")
        writeExecutable(
            ytDlpStub(root: root, title: "Single Format", behavior: formatUnavailableBehavior),
            named: "yt-dlp",
            in: root
        )
        let service = makeService(root: root, incoming: incoming)

        let jobID = try await service.start(url: URL(string: "https://example.com/v")!, maxHeight: 1080)
        let status = try #require(await waitForTerminal(service, jobID))

        #expect(status.state == .done)
        let path = try #require(status.path)
        #expect(path.lastPathComponent == "Single Format.mp4")
        #expect(FileManager.default.fileExists(atPath: path.path))

        let formats = recordedFormats(in: root)
        #expect(formats.first == DownloadFormatSelector.videoExpression(maxHeight: 1080, hwAV1: false))
        #expect(formats.last == DownloadFormatSelector.singleExpression(maxHeight: 1080))
        #expect(!formats.contains(DownloadFormatSelector.audioExpression()))
    }

    @Test func av1HardwarePreferenceReachesYtDlp() async throws {
        let root = makeTempDir("av1")
        let incoming = makeTempDir("av1-incoming")
        writeExecutable(ytDlpStub(root: root, title: "AV1 Clip", behavior: successBehavior), named: "yt-dlp", in: root)
        writeExecutable(ffmpegStub(root: root, succeed: true), named: "ffmpeg", in: root)
        let service = makeService(root: root, incoming: incoming, hwAV1: true)

        let jobID = try await service.start(url: URL(string: "https://example.com/v")!, maxHeight: 2160)
        _ = await waitForTerminal(service, jobID)

        #expect(recordedFormats(in: root).first?.contains("av01") == true)
    }
}

@Suite struct DownloadServiceLifecycleTests {
    @Test func muxFailureWipesJobFolder() async throws {
        let root = makeTempDir("muxfail")
        let incoming = makeTempDir("muxfail-incoming")
        writeExecutable(ytDlpStub(root: root, title: "Mux Clip", behavior: successBehavior), named: "yt-dlp", in: root)
        writeExecutable(ffmpegStub(root: root, succeed: false), named: "ffmpeg", in: root)
        let service = makeService(root: root, incoming: incoming)

        let jobID = try await service.start(url: URL(string: "https://example.com/v")!, maxHeight: 1080)
        let status = try #require(await waitForTerminal(service, jobID))

        #expect(status.state == .failed)
        #expect(status.error?.contains("mux failed") == true)
        #expect(status.path == nil)
        #expect(!FileManager.default.fileExists(atPath: jobDirectory(incoming: incoming, jobID: jobID).path))
    }

    @Test func downloadFailureWipesJobFolder() async throws {
        let root = makeTempDir("fail")
        let incoming = makeTempDir("fail-incoming")
        writeExecutable(ytDlpStub(root: root, title: "Broken", behavior: hardFailureBehavior), named: "yt-dlp", in: root)
        let service = makeService(root: root, incoming: incoming)

        let jobID = try await service.start(url: URL(string: "https://example.com/v")!, maxHeight: 1080)
        let status = try #require(await waitForTerminal(service, jobID))

        #expect(status.state == .failed)
        #expect(status.error?.contains("boom") == true)
        #expect(!FileManager.default.fileExists(atPath: jobDirectory(incoming: incoming, jobID: jobID).path))
    }

    @Test func cancelDiscardsJobFolderAndPartials() async throws {
        let root = makeTempDir("cancel")
        let incoming = makeTempDir("cancel-incoming")
        writeExecutable(ytDlpStub(root: root, title: "Long Clip", behavior: cancelBehavior), named: "yt-dlp", in: root)
        let service = makeService(root: root, incoming: incoming)

        let jobID = try await service.start(url: URL(string: "https://example.com/v")!, maxHeight: 1080)
        let marker = jobDirectory(incoming: incoming, jobID: jobID)
            .appendingPathComponent("video", isDirectory: true)
            .appendingPathComponent("started")
        #expect(await waitForFile(marker))

        await service.cancel(jobID)
        let status = try #require(await waitForTerminal(service, jobID))

        #expect(status.state == .canceled)
        #expect(status.path == nil)
        #expect(!FileManager.default.fileExists(atPath: jobDirectory(incoming: incoming, jobID: jobID).path))
    }

    @Test func titlePropagatesIntoStatus() async throws {
        let root = makeTempDir("title")
        let incoming = makeTempDir("title-incoming")
        writeExecutable(ytDlpStub(root: root, title: "My Clip", behavior: successBehavior), named: "yt-dlp", in: root)
        writeExecutable(ffmpegStub(root: root, succeed: true), named: "ffmpeg", in: root)
        let service = makeService(root: root, incoming: incoming)

        let jobID = try await service.start(url: URL(string: "https://example.com/v")!, maxHeight: 1080)
        let status = try #require(await waitForTerminal(service, jobID))

        #expect(status.title == "My Clip")
        #expect(status.path?.lastPathComponent == "My Clip.mp4")
    }

    @Test func collisionReceivesNumericSuffix() async throws {
        let root = makeTempDir("collision")
        let incoming = makeTempDir("collision-incoming")
        writeExecutable(ytDlpStub(root: root, title: "Dup", behavior: successBehavior), named: "yt-dlp", in: root)
        writeExecutable(ffmpegStub(root: root, succeed: true), named: "ffmpeg", in: root)
        let service = makeService(root: root, incoming: incoming)

        let firstID = try await service.start(url: URL(string: "https://example.com/a")!, maxHeight: 1080)
        let first = try #require(await waitForTerminal(service, firstID))
        let secondID = try await service.start(url: URL(string: "https://example.com/b")!, maxHeight: 1080)
        let second = try #require(await waitForTerminal(service, secondID))

        #expect(first.path?.lastPathComponent == "Dup.mp4")
        #expect(second.path?.lastPathComponent == "Dup-1.mp4")
        #expect(FileManager.default.fileExists(atPath: first.path!.path))
        #expect(FileManager.default.fileExists(atPath: second.path!.path))
    }

    @Test func recreateIncomingWipesEverything() async throws {
        let root = makeTempDir("recreate")
        let incoming = makeTempDir("recreate-incoming")
        let nested = incoming.appendingPathComponent("stale", isDirectory: true)
        try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: true)
        try Data("junk".utf8).write(to: nested.appendingPathComponent("partial.part"))
        let service = makeService(root: root, incoming: incoming)

        try await service.recreateIncoming()

        #expect(FileManager.default.fileExists(atPath: incoming.path))
        let contents = try FileManager.default.contentsOfDirectory(atPath: incoming.path)
        #expect(contents.isEmpty)
    }

    @Test func invalidURLIsRejected() async throws {
        let root = makeTempDir("invalid")
        let incoming = makeTempDir("invalid-incoming")
        let service = makeService(root: root, incoming: incoming)

        await #expect(throws: LiveWallError.ipc(code: "invalidURL", message: "unsupported download url")) {
            try await service.start(url: URL(string: "ftp://example.com/v")!, maxHeight: 1080)
        }
    }

    @Test func missingToolFailsJob() async throws {
        let root = makeTempDir("notool")
        let incoming = makeTempDir("notool-incoming")
        let service = makeService(root: root, incoming: incoming)

        let jobID = try await service.start(url: URL(string: "https://example.com/v")!, maxHeight: 1080)
        let status = try #require(await waitForTerminal(service, jobID))

        #expect(status.state == .failed)
        #expect(status.error?.contains("yt-dlp") == true)
    }

    @Test func unknownJobHasNoStatus() async throws {
        let root = makeTempDir("unknown")
        let incoming = makeTempDir("unknown-incoming")
        let service = makeService(root: root, incoming: incoming)
        #expect(await service.status(UUID()) == nil)
    }
}

@Suite struct DownloadServiceConcurrencyTests {
    @Test func parallelJobsDoNotSerialize() async throws {
        let root = makeTempDir("parallel")
        let incoming = makeTempDir("parallel-incoming")
        writeExecutable(ytDlpStub(root: root, title: "Parallel", behavior: slowBehavior), named: "yt-dlp", in: root)
        writeExecutable(ffmpegStub(root: root, succeed: true), named: "ffmpeg", in: root)
        let service = makeService(root: root, incoming: incoming)

        let firstID = try await service.start(url: URL(string: "https://example.com/a")!, maxHeight: 1080)
        let secondID = try await service.start(url: URL(string: "https://example.com/b")!, maxHeight: 1080)

        let firstMarker = jobDirectory(incoming: incoming, jobID: firstID)
            .appendingPathComponent("video", isDirectory: true).appendingPathComponent("started")
        let secondMarker = jobDirectory(incoming: incoming, jobID: secondID)
            .appendingPathComponent("video", isDirectory: true).appendingPathComponent("started")
        #expect(await waitForFile(firstMarker))
        #expect(await waitForFile(secondMarker))

        let first = try #require(await waitForTerminal(service, firstID))
        let second = try #require(await waitForTerminal(service, secondID))
        #expect(first.state == .done)
        #expect(second.state == .done)
    }

    @Test func eventsStreamReportsStartAndFinish() async throws {
        let root = makeTempDir("events")
        let incoming = makeTempDir("events-incoming")
        writeExecutable(ytDlpStub(root: root, title: "Event Clip", behavior: successBehavior), named: "yt-dlp", in: root)
        writeExecutable(ffmpegStub(root: root, succeed: true), named: "ffmpeg", in: root)
        let service = makeService(root: root, incoming: incoming)

        let stream = await service.events()
        let collector = Task { () -> [DownloadEvent] in
            var events: [DownloadEvent] = []
            for await event in stream {
                events.append(event)
                if case .finished = event {
                    break
                }
            }
            return events
        }

        let jobID = try await service.start(url: URL(string: "https://example.com/v")!, maxHeight: 1080)
        _ = await waitForTerminal(service, jobID)
        let events = await collector.value

        #expect(events.contains { if case .started = $0 { return true } else { return false } })
        #expect(events.contains { if case .progress = $0 { return true } else { return false } })
        #expect(events.contains { if case .finished = $0 { return true } else { return false } })
    }
}
