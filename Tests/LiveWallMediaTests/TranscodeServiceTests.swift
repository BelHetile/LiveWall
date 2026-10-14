import Foundation
import Testing
import LiveWallCore
import LiveWallTools
@testable import LiveWallMedia

private func makeTempDirectory(_ label: String) -> URL {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("lwtx-\(label)-\(UUID().uuidString)", isDirectory: true)
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

private func writeExecutable(_ script: String, named name: String, in directory: URL) {
    let url = directory.appendingPathComponent(name)
    try? script.write(to: url, atomically: true, encoding: .utf8)
    try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
}

private func video(
    codec: String = "av1",
    bitDepth: Int? = 8,
    width: Int? = 1920,
    height: Int? = 1080,
    frameRate: Double? = 30,
    colorPrimaries: String? = nil,
    colorTransfer: String? = nil,
    colorSpace: String? = nil
) -> VideoMetadata {
    return VideoMetadata(
        codec: codec,
        profile: nil,
        pixelFormat: bitDepth == 10 ? "yuv420p10le" : "yuv420p",
        bitDepth: bitDepth,
        colorSpace: colorSpace,
        colorPrimaries: colorPrimaries,
        colorTransfer: colorTransfer,
        hdrFormat: nil,
        width: width,
        height: height,
        frameRate: frameRate,
        frameCount: nil
    )
}

private func metadata(video: VideoMetadata?, audioCodec: String? = nil) -> MediaMetadata {
    let audio = audioCodec.map { AudioMetadata(codec: $0, profile: nil, channels: 2, sampleRate: 48000) }
    return MediaMetadata(container: "mov,mp4", duration: 2, bitRate: nil, video: video, audio: audio)
}

private func value(of flag: String, in arguments: [String]) -> String? {
    guard let index = arguments.firstIndex(of: flag), index + 1 < arguments.count else {
        return nil
    }
    return arguments[index + 1]
}

private let inputURL = URL(fileURLWithPath: "/tmp/in.mp4")
private let outputURL = URL(fileURLWithPath: "/tmp/out.mov")

private func buildArguments(
    video: VideoMetadata? = video(),
    audioCodec: String? = nil,
    maxHeight: Int = 720,
    fpsLimit: Int = 30,
    hevcAvailable: Bool = true,
    audioMode: TranscodeAudioMode = .drop
) -> [String] {
    return TranscodeArguments.arguments(
        input: inputURL,
        output: outputURL,
        source: metadata(video: video, audioCodec: audioCodec),
        parameters: TranscodeParameters(maxHeight: maxHeight, fpsLimit: fpsLimit),
        hevcAvailable: hevcAvailable,
        audioMode: audioMode
    )
}

@Suite struct TranscodeArgumentsTests {
    @Test func hevcEightBitDefaults() {
        let arguments = buildArguments()
        #expect(value(of: "-c:v", in: arguments) == "hevc_videotoolbox")
        #expect(value(of: "-pix_fmt", in: arguments) == "yuv420p")
        #expect(value(of: "-vf", in: arguments) == "scale=-2:min(ih\\,720)")
        #expect(!arguments.contains("-r"))
        #expect(value(of: "-b:v", in: arguments) == "2000000")
        #expect(value(of: "-tag:v", in: arguments) == "hvc1")
        #expect(!arguments.contains("-color_primaries"))
        #expect(arguments.last == outputURL.path)
        #expect(arguments.first == "-hide_banner")
    }

    @Test func tenBitHDRPreservesColorTags() {
        let arguments = buildArguments(
            video: video(
                bitDepth: 10,
                width: 3840,
                height: 2160,
                frameRate: 24,
                colorPrimaries: "bt2020",
                colorTransfer: "smpte2084",
                colorSpace: "bt2020nc"
            )
        )
        #expect(value(of: "-pix_fmt", in: arguments) == "yuv420p10le")
        #expect(value(of: "-color_primaries", in: arguments) == "bt2020")
        #expect(value(of: "-color_trc", in: arguments) == "smpte2084")
        #expect(value(of: "-colorspace", in: arguments) == "bt2020nc")
        #expect(value(of: "-b:v", in: arguments) == "2211840")
    }

    @Test func h264FallbackIsEightBitBT709() {
        let arguments = buildArguments(
            video: video(bitDepth: 10, width: 3840, height: 2160),
            hevcAvailable: false
        )
        #expect(value(of: "-c:v", in: arguments) == "h264_videotoolbox")
        #expect(value(of: "-pix_fmt", in: arguments) == "yuv420p")
        #expect(value(of: "-color_primaries", in: arguments) == "bt709")
        #expect(value(of: "-color_trc", in: arguments) == "bt709")
        #expect(value(of: "-colorspace", in: arguments) == "bt709")
        #expect(!arguments.contains("-tag:v"))
    }

    @Test func noScaleWhenSourceShorterThanTarget() {
        let arguments = buildArguments(video: video(width: 1600, height: 900, frameRate: 30), maxHeight: 1080)
        #expect(!arguments.contains("-vf"))
        #expect(value(of: "-b:v", in: arguments) == String(TranscodeArguments.videoBitrate(
            width: 1600,
            height: 900,
            bitDepth: 8,
            fps: 30
        )))
    }

    @Test func scaleCapsAtEvenTarget() {
        let arguments = buildArguments(video: video(width: 1920, height: 1081), maxHeight: 721)
        #expect(value(of: "-vf", in: arguments) == "scale=-2:min(ih\\,720)")
    }

    @Test func fpsCapOnlyWhenSourceExceeds() {
        #expect(value(of: "-r", in: buildArguments(video: video(frameRate: 60), fpsLimit: 30)) == "30")
        #expect(!buildArguments(video: video(frameRate: 30), fpsLimit: 30).contains("-r"))
        #expect(!buildArguments(video: video(frameRate: 24), fpsLimit: 30).contains("-r"))
    }

    @Test func bitrateUsesCappedFPS() {
        let arguments = buildArguments(video: video(width: 1920, height: 1080, frameRate: 60), fpsLimit: 30)
        #expect(value(of: "-b:v", in: arguments) == String(TranscodeArguments.videoBitrate(
            width: 1280,
            height: 720,
            bitDepth: 8,
            fps: 30
        )))
    }

    @Test func videoBitrateMathAndFloor() {
        #expect(TranscodeArguments.videoBitrate(width: 1920, height: 1080, bitDepth: 8, fps: 30) == 4_354_560)
        #expect(TranscodeArguments.videoBitrate(width: 1920, height: 1080, bitDepth: 10, fps: 30) == 6_220_800)
        #expect(TranscodeArguments.videoBitrate(width: 1280, height: 720, bitDepth: 8, fps: 30) == 2_000_000)
        #expect(TranscodeArguments.videoBitrate(width: nil, height: nil, bitDepth: 8, fps: 30) == 2_000_000)
        #expect(TranscodeArguments.videoBitrate(width: 0, height: 720, bitDepth: 8, fps: 30) == 2_000_000)
    }

    @Test func outputDimensionsKeepSourceWhenWithinTarget() {
        let dimensions = TranscodeArguments.outputDimensions(sourceWidth: 1920, sourceHeight: 1080, targetHeight: 1080)
        #expect(dimensions.width == 1920)
        #expect(dimensions.height == 1080)
    }

    @Test func outputDimensionsScaleEven() {
        let dimensions = TranscodeArguments.outputDimensions(sourceWidth: 1920, sourceHeight: 1080, targetHeight: 720)
        #expect(dimensions.width == 1280)
        #expect(dimensions.height == 720)
    }

    @Test func clampsFpsLimitRange() {
        #expect(TranscodeArguments.clampFpsLimit(5) == 15)
        #expect(TranscodeArguments.clampFpsLimit(200) == 120)
        #expect(TranscodeArguments.clampFpsLimit(30) == 30)
        #expect(TranscodeParameters(maxHeight: 720, fpsLimit: 1).fpsLimit == 15)
    }

    @Test func audioModeTable() {
        #expect(TranscodeArguments.audioMode(codec: "aac") == .passthrough)
        #expect(TranscodeArguments.audioMode(codec: "alac") == .passthrough)
        #expect(TranscodeArguments.audioMode(codec: "ac3") == .passthrough)
        #expect(TranscodeArguments.audioMode(codec: "eac3") == .passthrough)
        #expect(TranscodeArguments.audioMode(codec: "mp3") == .passthrough)
        #expect(TranscodeArguments.audioMode(codec: "pcm_s16le") == .passthrough)
        #expect(TranscodeArguments.audioMode(codec: "lpcm") == .passthrough)
        #expect(TranscodeArguments.audioMode(codec: "opus") == .reencode)
        #expect(TranscodeArguments.audioMode(codec: "vorbis") == .reencode)
        #expect(TranscodeArguments.audioMode(codec: nil) == .drop)
        #expect(TranscodeArguments.audioMode(codec: "") == .drop)
    }

    @Test func audioArgumentsTable() {
        #expect(TranscodeArguments.audioArguments(.passthrough) == ["-c:a", "copy"])
        #expect(TranscodeArguments.audioArguments(.reencode) == ["-c:a", "aac", "-b:a", "176k", "-ar", "48000", "-ac", "2"])
        #expect(TranscodeArguments.audioArguments(.drop) == ["-an"])
        #expect(value(of: "-c:a", in: buildArguments(audioCodec: "aac", audioMode: .passthrough)) == "copy")
        #expect(value(of: "-c:a", in: buildArguments(audioCodec: "opus", audioMode: .reencode)) == "aac")
        #expect(buildArguments(audioCodec: nil, audioMode: .drop).contains("-an"))
    }
}

private func makeTranscodeService(root: URL, hevc: Bool = true) -> TranscodeService {
    return TranscodeService(resolver: ToolLocator(searchDirectories: [root]), hevcAvailable: { hevc })
}

private func successStub() -> String {
    return """
    #!/bin/sh
    last=""
    for a in "$@"; do last="$a"; done
    : > "$last"
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

private func audioFallbackStub(root: URL) -> String {
    let log = root.appendingPathComponent("ffmpeg-calls.txt").path
    return """
    #!/bin/sh
    printf '%s\\n' "$*" >> "\(log)"
    has_an=0
    for a in "$@"; do
      if [ "$a" = "-an" ]; then has_an=1; fi
    done
    if [ "$has_an" = "0" ]; then
      printf 'audio decode failed\\n' >&2
      exit 1
    fi
    last=""
    for a in "$@"; do last="$a"; done
    : > "$last"
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

private func callCount(root: URL) -> Int {
    guard let contents = try? String(contentsOf: root.appendingPathComponent("ffmpeg-calls.txt"), encoding: .utf8) else {
        return 0
    }
    return contents.split(separator: "\n").count
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

@Suite struct TranscodeServiceTests {
    @Test func stubTranscodeProducesOutput() async throws {
        let root = makeTempDirectory("tx-ok")
        writeExecutable(successStub(), named: "ffmpeg", in: root)
        let service = makeTranscodeService(root: root)
        let input = root.appendingPathComponent("in.mp4")
        try Data("x".utf8).write(to: input)
        let output = root.appendingPathComponent("out.mov")

        let result = try await service.transcode(
            input: input,
            output: output,
            source: metadata(video: video()),
            parameters: TranscodeParameters(maxHeight: 720)
        )

        #expect(result.outputURL == output)
        #expect(result.videoCodec == "hevc")
        #expect(result.audio == .dropped(message: "added without sound"))
        #expect(result.audio.message == "added without sound")
        #expect(FileManager.default.fileExists(atPath: output.path))
    }

    @Test func h264FallbackReportsCodec() async throws {
        let root = makeTempDirectory("tx-h264")
        writeExecutable(successStub(), named: "ffmpeg", in: root)
        let service = makeTranscodeService(root: root, hevc: false)
        let input = root.appendingPathComponent("in.mp4")
        try Data("x".utf8).write(to: input)
        let output = root.appendingPathComponent("out.mov")

        let result = try await service.transcode(
            input: input,
            output: output,
            source: metadata(video: video()),
            parameters: TranscodeParameters(maxHeight: 720)
        )
        #expect(result.videoCodec == "h264")
    }

    @Test func undecodableAudioFallsBackToDropWithMessage() async throws {
        let root = makeTempDirectory("tx-audio")
        writeExecutable(audioFallbackStub(root: root), named: "ffmpeg", in: root)
        let service = makeTranscodeService(root: root)
        let input = root.appendingPathComponent("in.mkv")
        try Data("x".utf8).write(to: input)
        let output = root.appendingPathComponent("out.mov")

        let result = try await service.transcode(
            input: input,
            output: output,
            source: metadata(video: video(), audioCodec: "opus"),
            parameters: TranscodeParameters(maxHeight: 720)
        )

        #expect(result.audio == .dropped(message: "added without sound"))
        #expect(callCount(root: root) == 2)
        #expect(FileManager.default.fileExists(atPath: output.path))
    }

    @Test func passthroughAudioIsNotRetried() async throws {
        let root = makeTempDirectory("tx-passthrough")
        let stub = """
        #!/bin/sh
        printf 'boom\\n' >&2
        exit 1
        """
        writeExecutable(stub, named: "ffmpeg", in: root)
        let service = makeTranscodeService(root: root)
        let input = root.appendingPathComponent("in.mp4")
        try Data("x".utf8).write(to: input)
        let output = root.appendingPathComponent("out.mov")

        await #expect(throws: LiveWallError.transcodeFailed(reason: "boom\n")) {
            try await service.transcode(
                input: input,
                output: output,
                source: metadata(video: video(), audioCodec: "aac"),
                parameters: TranscodeParameters(maxHeight: 720)
            )
        }
        #expect(!FileManager.default.fileExists(atPath: output.path))
    }

    @Test func cancelDeletesPartialOutput() async throws {
        let root = makeTempDirectory("tx-cancel")
        writeExecutable(slowStub(root: root), named: "ffmpeg", in: root)
        let service = makeTranscodeService(root: root)
        let input = root.appendingPathComponent("in.mp4")
        try Data("x".utf8).write(to: input)
        let output = root.appendingPathComponent("out.mov")

        let task = Task {
            try await service.transcode(
                input: input,
                output: output,
                source: metadata(video: video()),
                parameters: TranscodeParameters(maxHeight: 720)
            )
        }
        #expect(await waitForFile(root.appendingPathComponent("started")))
        await service.cancelActive()

        await #expect(throws: CancellationError.self) {
            _ = try await task.value
        }
        #expect(!FileManager.default.fileExists(atPath: output.path))
    }

    @Test func missingFFmpegThrowsToolMissing() async throws {
        let root = makeTempDirectory("tx-notool")
        let service = makeTranscodeService(root: root)
        let input = root.appendingPathComponent("in.mp4")
        try Data("x".utf8).write(to: input)
        let output = root.appendingPathComponent("out.mov")

        await #expect(throws: LiveWallError.toolMissing(tool: "ffmpeg")) {
            try await service.transcode(
                input: input,
                output: output,
                source: metadata(video: video()),
                parameters: TranscodeParameters(maxHeight: 720)
            )
        }
    }
}
