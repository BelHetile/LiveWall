import Foundation
import Testing
import LiveWallCore
import LiveWallTools
@testable import LiveWallMedia

private let repoRoot = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent()
    .deletingLastPathComponent()
    .deletingLastPathComponent()
private let fixturesDirectory = repoRoot.appendingPathComponent("Tests/Fixtures", isDirectory: true)
private let cannedDirectory = fixturesDirectory.appendingPathComponent("ffprobe", isDirectory: true)

private func fixture(_ name: String) -> URL {
    return fixturesDirectory.appendingPathComponent(name)
}

private func cannedJSON(_ name: String) throws -> String {
    return try String(contentsOf: cannedDirectory.appendingPathComponent("\(name).json"), encoding: .utf8)
}

private func makeProbeDir(_ label: String) -> URL {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("lwprobe-\(label)-\(UUID().uuidString)", isDirectory: true)
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

private func writeExecutable(_ script: String, named name: String, in directory: URL) {
    let url = directory.appendingPathComponent(name)
    try? script.write(to: url, atomically: true, encoding: .utf8)
    try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
}

private func makeProbe(root: URL, av1Hardware: Bool = false) -> MediaProbeService {
    let stub = """
    #!/bin/sh
    last=""
    for a in "$@"; do last="$a"; done
    base=$(basename "$last")
    if [ "$base" = "zero-bytes.mp4" ]; then exit 1; fi
    f="\(cannedDirectory.path)/$base.json"
    if [ -f "$f" ]; then cat "$f"; exit 0; fi
    exit 1
    """
    writeExecutable(stub, named: "ffprobe", in: root)
    return MediaProbeService(
        resolver: ToolLocator(searchDirectories: [root]),
        av1HardwareSupport: { av1Hardware }
    )
}

@Suite struct FFProbeMetadataParsingTests {
    @Test func parsesHEVCClip() throws {
        let metadata = try FFProbeMetadataParser.parse(json: cannedJSON("clip-hevc-1080p30.mp4"))
        #expect(metadata.container == "mov,mp4,m4a,3gp,3g2,mj2")
        #expect(metadata.duration == 2.0)
        #expect(metadata.video?.codec == "hevc")
        #expect(metadata.video?.pixelFormat == "yuv420p")
        #expect(metadata.video?.bitDepth == 8)
        #expect(metadata.video?.width == 1920)
        #expect(metadata.video?.height == 1080)
        #expect(metadata.video?.frameRate == 30)
        #expect(metadata.video?.frameCount == 60)
        #expect(metadata.video?.hdrFormat == nil)
        #expect(metadata.audio?.codec == "aac")
        #expect(metadata.audio?.channels == 1)
        #expect(metadata.audio?.sampleRate == 48000)
    }

    @Test func parsesAV1Clip() throws {
        let metadata = try FFProbeMetadataParser.parse(json: cannedJSON("clip-av1-1080p.mp4"))
        #expect(metadata.video?.codec == "av1")
        #expect(metadata.video?.frameRate == 30)
        #expect(metadata.video?.frameCount == 60)
    }

    @Test func detectsPQTenBitHDR() throws {
        let json = """
        {"streams":[{"codec_name":"hevc","codec_type":"video","profile":"Main 10",
        "pix_fmt":"yuv420p10le","color_transfer":"smpte2084","color_primaries":"bt2020",
        "color_space":"bt2020nc","width":3840,"height":2160,"avg_frame_rate":"24/1",
        "nb_frames":"48"}],"format":{"format_name":"mov,mp4","duration":"2.000000","bit_rate":"1000000"}}
        """
        let metadata = try FFProbeMetadataParser.parse(json: json)
        #expect(metadata.video?.bitDepth == 10)
        #expect(metadata.video?.hdrFormat == .pq)
        #expect(metadata.video?.colorPrimaries == "bt2020")
        #expect(metadata.video?.colorTransfer == "smpte2084")
        #expect(metadata.video?.frameRate == 24)
        #expect(metadata.bitRate == 1000000)
    }

    @Test func detectsHLG() throws {
        let json = """
        {"streams":[{"codec_name":"hevc","codec_type":"video","pix_fmt":"yuv420p10le",
        "color_transfer":"arib-std-b67","color_primaries":"bt2020","color_space":"bt2020nc",
        "width":1920,"height":1080,"avg_frame_rate":"60000/1001"}],"format":{"format_name":"mpegts","duration":"1.500000"}}
        """
        let metadata = try FFProbeMetadataParser.parse(json: json)
        #expect(metadata.video?.hdrFormat == .hlg)
        #expect(metadata.video?.bitDepth == 10)
        #expect(abs((metadata.video?.frameRate ?? 0) - 59.94) < 0.01)
    }

    @Test func parsesAudioOnlyWithNoVideo() throws {
        let metadata = try FFProbeMetadataParser.parse(json: cannedJSON("audio.m4a"))
        #expect(metadata.video == nil)
        #expect(metadata.audio?.codec == "aac")
        #expect(metadata.audio?.channels == 1)
        #expect(metadata.audio?.sampleRate == 48000)
    }

    @Test func parsesMultiAudioVariant() throws {
        let json = """
        {"streams":[{"codec_name":"h264","codec_type":"video","pix_fmt":"yuv420p","width":1920,"height":1080,"avg_frame_rate":"30/1"},
        {"codec_name":"eac3","codec_type":"audio","channels":6,"sample_rate":"48000","profile":"Dolby Digital Plus"},
        {"codec_name":"aac","codec_type":"audio","channels":2,"sample_rate":"44100"}],
        "format":{"format_name":"matroska,webm","duration":"3.000000"}}
        """
        let metadata = try FFProbeMetadataParser.parse(json: json)
        #expect(metadata.video?.codec == "h264")
        #expect(metadata.audio?.codec == "eac3")
        #expect(metadata.audio?.channels == 6)
        #expect(metadata.container == "matroska,webm")
    }

    @Test func rejectsUnrecognizedOutput() {
        #expect(throws: LiveWallError.self) {
            try FFProbeMetadataParser.parse(json: "not json")
        }
    }
}

@Suite struct MediaProbeVerdictTests {
    @Test func av1OnM1NeedsTranscode() async throws {
        let probe = makeProbe(root: makeProbeDir("av1-m1"), av1Hardware: false)
        let result = try await probe.probe(fixture("clip-av1-1080p.mp4"))
        #expect(result.kind == .video)
        guard case let .transcodeToHEVC(warning) = result.verdict else {
            Issue.record("expected transcodeToHEVC, got \(result.verdict)")
            return
        }
        #expect(warning == MediaProbeService.av1TranscodeWarning)
    }

    @Test func av1OnM3IsNative() async throws {
        let probe = makeProbe(root: makeProbeDir("av1-m3"), av1Hardware: true)
        let result = try await probe.probe(fixture("clip-av1-1080p.mp4"))
        #expect(result.verdict == .native)
    }

    @Test func hevcClipIsNative() async throws {
        let probe = makeProbe(root: makeProbeDir("hevc"))
        let result = try await probe.probe(fixture("clip-hevc-1080p30.mp4"))
        #expect(result.kind == .video)
        #expect(result.verdict == .native)
        #expect(result.metadata?.video?.codec == "hevc")
    }

    @Test func h264ClipIsNative() async throws {
        let probe = makeProbe(root: makeProbeDir("h264"))
        let result = try await probe.probe(fixture("clip-h264-720p60.mp4"))
        #expect(result.kind == .video)
        #expect(result.verdict == .native)
    }

    @Test func gifIsAnimatedImage() async throws {
        let probe = makeProbe(root: makeProbeDir("gif"))
        let result = try await probe.probe(fixture("anim.gif"))
        #expect(result.kind == .animatedImage)
        #expect(result.verdict == .native)
    }

    @Test func pngIsImage() async throws {
        let probe = makeProbe(root: makeProbeDir("png"))
        let result = try await probe.probe(fixture("still.png"))
        #expect(result.kind == .image)
        #expect(result.verdict == .native)
    }

    @Test func audioOnlyIsRejected() async throws {
        let probe = makeProbe(root: makeProbeDir("audio"))
        let result = try await probe.probe(fixture("audio.m4a"))
        guard case let .rejected(reason) = result.verdict else {
            Issue.record("expected rejection, got \(result.verdict)")
            return
        }
        #expect(reason.contains("audio-only"))
    }

    @Test func corruptFileIsRejected() async throws {
        let probe = makeProbe(root: makeProbeDir("corrupt"))
        let result = try await probe.probe(fixture("corrupt.mp4"))
        guard case .rejected = result.verdict else {
            Issue.record("expected rejection, got \(result.verdict)")
            return
        }
    }

    @Test func zeroByteFileIsRejected() async throws {
        let probe = makeProbe(root: makeProbeDir("zero"))
        let result = try await probe.probe(fixture("zero-bytes.mp4"))
        guard case let .rejected(reason) = result.verdict else {
            Issue.record("expected rejection, got \(result.verdict)")
            return
        }
        #expect(reason.contains("empty"))
    }

    @Test func unsupportedContainerIsRejected() async throws {
        let probe = makeProbe(root: makeProbeDir("avi"))
        let url = makeProbeDir("avi-file").appendingPathComponent("video.avi")
        try Data(repeating: 0, count: 64).write(to: url)
        let result = try await probe.probe(url)
        guard case let .rejected(reason) = result.verdict else {
            Issue.record("expected rejection, got \(result.verdict)")
            return
        }
        #expect(reason.contains("unsupported container"))
    }

    @Test func probeOrThrowSurfacesTypedError() async throws {
        let probe = makeProbe(root: makeProbeDir("throw"))
        await #expect(throws: LiveWallError.self) {
            try await probe.probeOrThrow(fixture("audio.m4a"))
        }
    }

    @Test func missingFFProbeSurfacesToolMissing() async throws {
        let probe = MediaProbeService(resolver: ToolLocator(searchDirectories: [makeProbeDir("no-tool")]))
        await #expect(throws: LiveWallError.toolMissing(tool: "ffprobe")) {
            try await probe.probe(fixture("clip-hevc-1080p30.mp4"))
        }
    }
}
