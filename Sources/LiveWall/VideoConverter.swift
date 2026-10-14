import AVFoundation
import AudioToolbox
import CoreMedia
import VideoToolbox

enum ConvertError: LocalizedError {
    case unsupportedCodec(String)
    case decoder(String)
    case encoder
    case badFile
    case audio

    var errorDescription: String? {
        switch self {
        case .unsupportedCodec(let name):
            if name == "VP8" { return L("VP8 video can't be decoded on a Mac. Try a VP9, AV1 or MP4 version of this video.") }
            return LF("The %@ codec is not supported.", name)
        case .decoder(let name):
            if name == "AV1" { return L("This Mac can't decode AV1 video (it needs an M3 chip or newer).") }
            return LF("This Mac can't decode %@ video.", name)
        case .encoder: return L("Could not write the converted video.")
        case .badFile: return L("The file is damaged or not a video.")
        case .audio: return L("Could not add the sound track to the video.")
        }
    }
}

struct ConvertReport {
    let audioDropped: Bool
}

/// A source of compressed frames (in decode order) for VideoToolbox.
private protocol PacketSource: AnyObject {
    var format: CMVideoFormatDescription { get }
    var duration: Double { get }
    var transform: CGAffineTransform { get }
    var codecName: String { get }
    var colorInfo: VideoColorInfo { get }
    func next() -> CMSampleBuffer?
}

/// Converts videos macOS can't play (WebM, MKV, VP9/AV1 in MP4) to regular HEVC.
enum VideoConverter {
    static let vp9: CMVideoCodecType = 0x7670_3039   // 'vp09'
    static let av1: CMVideoCodecType = 0x6176_3031   // 'av01'

    /// Whether macOS (AVFoundation) can play the file by itself.
    static func canPlayNatively(_ url: URL) async -> Bool {
        let ext = url.pathExtension.lowercased()
        if ["webm", "mkv"].contains(ext) { return false }
        let asset = AVURLAsset(url: url)
        guard let track = try? await asset.loadTracks(withMediaType: .video).first else { return false }
        let playable = (try? await asset.load(.isPlayable)) ?? false
        let decodable = (try? await track.load(.isDecodable)) ?? false
        return playable && decodable
    }

    @discardableResult
    static func convert(source: URL, dest: URL, fpsLimit: Double, target: CGSize,
                        token: CancelToken, progress: @escaping (Double) -> Void) async throws -> ConvertReport {
        VTRegisterSupplementalVideoDecoderIfAvailable(vp9)
        do {
            return try await run(source: source, dest: dest, fpsLimit: fpsLimit, target: target,
                                 codec: .hevc, token: token, progress: progress)
        } catch ConvertError.encoder {
            // no HEVC encoder (rare): fall back to H.264
            return try await run(source: source, dest: dest, fpsLimit: fpsLimit, target: target,
                                 codec: .h264, token: token, progress: progress)
        }
    }

    private static func makeSource(_ url: URL) async throws -> PacketSource {
        let ext = url.pathExtension.lowercased()
        if ["webm", "mkv"].contains(ext) { return try MatroskaSource(url: url) }
        if let source = try? await AssetPacketSource.make(url: url) { return source }
        // the extension may be wrong; maybe it's Matroska inside
        return try MatroskaSource(url: url)
    }

    private final class FrameBox {
        let lock = NSLock()
        var frames: [(CMTime, CVPixelBuffer)] = []
        var failures = 0

        func take() -> [(CMTime, CVPixelBuffer)] {
            lock.lock(); defer { lock.unlock() }
            let out = frames
            frames.removeAll()
            return out
        }
    }

    private static func run(source url: URL, dest: URL, fpsLimit: Double, target: CGSize, codec: AVVideoCodecType,
                            token: CancelToken, progress: @escaping (Double) -> Void) async throws -> ConvertReport {
        let source = try await makeSource(url)
        let format = source.format
        let color = source.colorInfo
        let dims = CMVideoFormatDescriptionGetDimensions(format)
        guard dims.width > 0, dims.height > 0 else { throw ConvertError.badFile }

        let use10bit = codec == .hevc && color.bitDepth > 8

        // decoder
        var session: VTDecompressionSession?
        let attrs: [String: Any] = [
            kCVPixelBufferPixelFormatTypeKey as String: use10bit ? kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange
                                                                : kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            kCVPixelBufferIOSurfacePropertiesKey as String: [String: Any](),
        ]
        var status = VTDecompressionSessionCreate(allocator: nil, formatDescription: format, decoderSpecification: nil,
                                                  imageBufferAttributes: attrs as CFDictionary, outputCallback: nil,
                                                  decompressionSessionOut: &session)
        if status != noErr || session == nil {
            status = VTDecompressionSessionCreate(allocator: nil, formatDescription: format, decoderSpecification: nil,
                                                  imageBufferAttributes: nil, outputCallback: nil,
                                                  decompressionSessionOut: &session)
        }
        guard status == noErr, let decoder = session else { throw ConvertError.decoder(source.codecName) }
        defer { VTDecompressionSessionInvalidate(decoder) }

        // size: cover the screen, but never upscale
        let natural = CGSize(width: CGFloat(dims.width), height: CGFloat(dims.height))
        let shown = CGRect(origin: .zero, size: natural).applying(source.transform)
        let scale = min(1, max(target.width / max(abs(shown.width), 1), target.height / max(abs(shown.height), 1)))
        let outW = max(2, Int((natural.width * scale / 2).rounded()) * 2)
        let outH = max(2, Int((natural.height * scale / 2).rounded()) * 2)
        let bitsPerPixel = use10bit ? 0.10 : (codec == .hevc ? 0.07 : 0.12)
        let bitrate = max(2_000_000, Int(Double(outW * outH) * fpsLimit * bitsPerPixel))

        let outputColor: VideoColorInfo = codec == .h264 && color.isHDR ? VideoColorInfo() : color
        var compression: [String: Any] = [
            AVVideoAverageBitRateKey: bitrate,
            AVVideoExpectedSourceFrameRateKey: Int(fpsLimit.rounded()),
            AVVideoMaxKeyFrameIntervalKey: Int(fpsLimit.rounded()) * 2,
        ]
        if use10bit { compression[AVVideoProfileLevelKey] = kVTProfileLevel_HEVC_Main10_AutoLevel }

        try? FileManager.default.removeItem(at: dest)
        guard let writer = try? AVAssetWriter(outputURL: dest, fileType: .mov) else { throw ConvertError.encoder }
        let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: codec,
            AVVideoWidthKey: outW,
            AVVideoHeightKey: outH,
            AVVideoScalingModeKey: AVVideoScalingModeResizeAspectFill,
            AVVideoColorPropertiesKey: outputColor.tag,
            AVVideoCompressionPropertiesKey: compression,
        ])
        input.transform = source.transform
        input.expectsMediaDataInRealTime = false
        let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: nil)
        guard writer.canAdd(input) else { throw ConvertError.encoder }
        writer.add(input)
        guard writer.startWriting() else { throw ConvertError.encoder }
        writer.startSession(atSourceTime: .zero)

        func failWith(_ error: Error) -> Error {
            writer.cancelWriting()
            try? FileManager.default.removeItem(at: dest)
            return error
        }

        let duration = source.duration
        let minStep = 1.0 / fpsLimit - 0.002
        var firstPts: CMTime?
        var lastWritten = -Double.infinity
        var nextKeep = -Double.infinity
        var written = 0

        func emit(_ pts: CMTime, _ buffer: CVPixelBuffer) throws {
            if firstPts == nil { firstPts = pts }
            let t = CMTimeGetSeconds(CMTimeSubtract(pts, firstPts!))
            guard t.isFinite, t + 0.0001 >= nextKeep, t > lastWritten else { return }
            while !input.isReadyForMoreMediaData {
                if token.isCancelled { throw OptimizeError.cancelled }
                if writer.status == .failed { throw ConvertError.encoder }
                usleep(2000)
            }
            guard adaptor.append(buffer, withPresentationTime: CMTime(seconds: t, preferredTimescale: 90_000)) else {
                throw ConvertError.encoder
            }
            lastWritten = t
            nextKeep = t + minStep
            written += 1
            if duration > 0 { progress(min(0.99, max(0, t / duration))) }
        }

        // the decoder returns frames in decode order; a small buffer sorts them by presentation time
        var pending: [(CMTime, CVPixelBuffer)] = []
        func absorb(_ frames: [(CMTime, CVPixelBuffer)], flush: Bool) throws {
            for f in frames where f.0.isValid {
                let i = pending.firstIndex(where: { CMTimeCompare($0.0, f.0) > 0 }) ?? pending.count
                pending.insert(f, at: i)
            }
            while pending.count > (flush ? 0 : 8) {
                let f = pending.removeFirst()
                try emit(f.0, f.1)
            }
        }

        let box = FrameBox()
        var submitted = 0
        do {
            while let sample = source.next() {
                if token.isCancelled { throw OptimizeError.cancelled }
                submitted += 1
                let st = VTDecompressionSessionDecodeFrame(decoder, sampleBuffer: sample, flags: [],
                                                           infoFlagsOut: nil) { status, _, image, pts, _ in
                    box.lock.lock()
                    if status == noErr, let image = image {
                        box.frames.append((pts, image))
                    } else if status != noErr {
                        box.failures += 1
                    }
                    box.lock.unlock()
                }
                if st != noErr { box.lock.lock(); box.failures += 1; box.lock.unlock() }
                try absorb(box.take(), flush: false)
                // no frames after dozens of attempts: the decoder can't handle it
                if written == 0 && pending.isEmpty && box.failures > 30 { throw ConvertError.decoder(source.codecName) }
            }
            VTDecompressionSessionFinishDelayedFrames(decoder)
            VTDecompressionSessionWaitForAsynchronousFrames(decoder)
            try absorb(box.take(), flush: true)
        } catch {
            throw failWith(error)
        }

        guard written > 0 else {
            throw failWith(submitted == 0 ? ConvertError.badFile : ConvertError.decoder(source.codecName))
        }
        input.markAsFinished()
        let end = max(lastWritten + 1.0 / fpsLimit, duration > 0 ? min(duration, lastWritten + 1) : 0)
        writer.endSession(atSourceTime: CMTime(seconds: end, preferredTimescale: 90_000))
        await writer.finishWriting()
        guard writer.status == .completed else {
            try? FileManager.default.removeItem(at: dest)
            throw ConvertError.encoder
        }
        progress(1)
        let videoStart = firstPts.map { CMTimeGetSeconds($0) } ?? 0
        let attach: AudioAttach
        do {
            attach = try await attachAudio(source: url, video: dest, videoStart: videoStart, token: token)
        } catch {
            try? FileManager.default.removeItem(at: dest)
            throw error
        }
        return ConvertReport(audioDropped: attach == .dropped)
    }
}

// MARK: - MP4 / MOV with a codec AVFoundation can't decode by itself

private final class AssetPacketSource: PacketSource {
    let reader: AVAssetReader
    let output: AVAssetReaderTrackOutput
    let format: CMVideoFormatDescription
    let duration: Double
    let transform: CGAffineTransform
    let codecName: String
    let colorInfo: VideoColorInfo

    private init(reader: AVAssetReader, output: AVAssetReaderTrackOutput, format: CMVideoFormatDescription,
                 duration: Double, transform: CGAffineTransform) {
        self.reader = reader
        self.output = output
        self.format = format
        self.duration = duration
        self.transform = transform
        codecName = VideoOptimizer.codecName(CMFormatDescriptionGetMediaSubType(format))
        colorInfo = VideoColorInfo.read(format)
    }

    static func make(url: URL) async throws -> AssetPacketSource {
        let asset = AVURLAsset(url: url)
        guard let track = try await asset.loadTracks(withMediaType: .video).first,
              let format = try await track.load(.formatDescriptions).first else { throw ConvertError.badFile }
        let transform = try await track.load(.preferredTransform)
        let duration = CMTimeGetSeconds(try await asset.load(.duration))
        let reader = try AVAssetReader(asset: asset)
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
        output.alwaysCopiesSampleData = false
        guard reader.canAdd(output) else { throw ConvertError.badFile }
        reader.add(output)
        guard reader.startReading() else { throw ConvertError.badFile }
        return AssetPacketSource(reader: reader, output: output, format: format,
                                 duration: duration.isFinite ? duration : 0, transform: transform)
    }

    func next() -> CMSampleBuffer? {
        output.copyNextSampleBuffer()
    }
}

// MARK: - WebM / Matroska

private final class MatroskaSource: PacketSource {
    private struct Packet {
        let offset: Int
        let length: Int
        let pts: Double
        let key: Bool
    }

    struct AudioPacket {
        let offset: Int
        let length: Int
        let pts: Double
    }

    struct AudioTrack {
        let codecID: String
        let codecPrivate: Data
        let sampleRate: Double
        let channels: Int
        let framesPerPacket: UInt32
        let preSkip: Int

        static let vorbisFormatID: AudioFormatID = 0x766F_7262

        func streamDescription(rate: Double, channels: Int) -> AudioStreamBasicDescription {
            var d = AudioStreamBasicDescription()
            switch codecID {
            case "A_OPUS": d.mFormatID = kAudioFormatOpus
            case "A_VORBIS": d.mFormatID = AudioTrack.vorbisFormatID
            default: d.mFormatID = kAudioFormatMPEG4AAC
            }
            d.mSampleRate = rate
            d.mChannelsPerFrame = UInt32(max(1, channels))
            d.mFramesPerPacket = framesPerPacket
            return d
        }
    }

    private let data: Data
    private var packets: [Packet] = []
    private var index = 0
    private var timecodeScale = 1_000_000.0
    private var durationTicks: Double?
    private var trackNumber: UInt64?
    private var codecID = ""
    private var codecPrivate = Data()
    private var width = 0
    private var height = 0
    private var colourMatrix = 0
    private var colourTransfer = 0
    private var colourPrimaries = 0
    private var colourBits = 0

    private var audioTrackNumber: UInt64?
    private var audioCodecID = ""
    private var audioCodecPrivate = Data()
    private var audioSampleRate = 48_000.0
    private var audioChannels = 2
    var audioPackets: [AudioPacket] = []

    var byteCount: Int { data.count }
    var hasAudio: Bool { audioTrackNumber != nil }

    var audioTrack: AudioTrack? {
        guard audioTrackNumber != nil else { return nil }
        switch audioCodecID {
        case "A_OPUS":
            let priv = audioCodecPrivate
            guard priv.count >= 19, Array(priv.prefix(8)) == Array("OpusHead".utf8) else { return nil }
            let base = priv.startIndex
            let channels = Int(priv[base + 9])
            let preSkip = Int(priv[base + 10]) | Int(priv[base + 11]) << 8
            return AudioTrack(codecID: audioCodecID, codecPrivate: priv, sampleRate: 48_000,
                              channels: max(1, channels), framesPerPacket: 960, preSkip: preSkip)
        case "A_AAC":
            guard let config = MatroskaSource.aacConfig(audioCodecPrivate) else { return nil }
            return AudioTrack(codecID: audioCodecID, codecPrivate: audioCodecPrivate, sampleRate: config.rate,
                              channels: config.channels, framesPerPacket: 1024,
                              preSkip: Int((1024 * 48_000 / config.rate).rounded()))
        case "A_VORBIS":
            guard let setup = MatroskaSource.vorbisSetup(audioCodecPrivate) else { return nil }
            return AudioTrack(codecID: audioCodecID, codecPrivate: setup.cookie, sampleRate: setup.rate,
                              channels: setup.channels, framesPerPacket: 0, preSkip: 0)
        default:
            return nil
        }
    }

    private static func vorbisSetup(_ priv: Data) -> (cookie: Data, rate: Double, channels: Int)? {
        let bytes = [UInt8](priv)
        guard bytes.count > 3 else { return nil }
        var pos = 1
        var idLength = 0
        while pos < bytes.count, bytes[pos] == 0xFF {
            idLength += 255
            pos += 1
        }
        guard pos < bytes.count else { return nil }
        idLength += Int(bytes[pos])
        pos += 1
        while pos < bytes.count, bytes[pos] == 0xFF { pos += 1 }
        pos += 1
        guard idLength >= 16, pos + idLength <= bytes.count,
              bytes[pos] == 1, Array(bytes[(pos + 1)..<(pos + 7)]) == Array("vorbis".utf8) else { return nil }
        let channels = Int(bytes[pos + 11])
        let rate = Double(UInt32(bytes[pos + 12]) | UInt32(bytes[pos + 13]) << 8
                          | UInt32(bytes[pos + 14]) << 16 | UInt32(bytes[pos + 15]) << 24)
        guard channels > 0, rate > 0 else { return nil }
        var cookie = priv
        cookie[cookie.startIndex] = 2
        return (cookie, rate, channels)
    }

    func withAudioBytes<T>(_ body: (UnsafeRawPointer) throws -> T) rethrows -> T {
        try data.withUnsafeBytes { raw in
            guard let base = raw.baseAddress else { throw ConvertError.badFile }
            return try body(base)
        }
    }

    private(set) var format: CMVideoFormatDescription
    private var storedDuration: Double = 0
    private var storedCodecName = ""
    private var finished = false
    var duration: Double { storedDuration }
    var codecName: String { storedCodecName }
    let transform = CGAffineTransform.identity

    var colorInfo: VideoColorInfo {
        var info = VideoColorInfo()
        switch colourPrimaries {
        case 9: info.primaries = AVVideoColorPrimaries_ITU_R_2020
        case 5, 6: info.primaries = AVVideoColorPrimaries_SMPTE_C
        default: break
        }
        switch colourTransfer {
        case 16: info.transfer = AVVideoTransferFunction_SMPTE_ST_2084_PQ
        case 18: info.transfer = AVVideoTransferFunction_ITU_R_2100_HLG
        default: break
        }
        switch colourMatrix {
        case 9: info.matrix = AVVideoYCbCrMatrix_ITU_R_2020
        case 5, 6: info.matrix = AVVideoYCbCrMatrix_ITU_R_601_4
        default: break
        }
        let bits = colourBits > 0 ? colourBits : codecBitDepth()
        if bits > 0 { info.bitDepth = bits }
        if info.isHDR && info.bitDepth < 10 { info.bitDepth = 10 }
        return info
    }

    private func codecBitDepth() -> Int {
        switch codecID {
        case "V_VP9": return vp9Info()?.bitDepth ?? 0
        case "V_AV1": return MatroskaSource.av1BitDepth(codecPrivate)
        default: return 0
        }
    }

    private static func av1BitDepth(_ config: Data) -> Int {
        guard config.count >= 3 else { return 0 }
        let base = config.startIndex
        let highBitDepth = (config[base + 2] >> 6) & 1
        let twelveBit = (config[base + 2] >> 5) & 1
        if highBitDepth == 0 { return 8 }
        return twelveBit == 1 ? 12 : 10
    }

    // segment top-level elements: a cluster of unknown size ends at one of them
    private static let topLevel: Set<UInt64> = [0x1F43_B675, 0x1C53_BB6B, 0x1254_C367, 0x1549_A966,
                                                0x1654_AE6B, 0x114D_9B74, 0x1043_A770, 0x1941_A469]

    init(url: URL) throws {
        data = try Data(contentsOf: url, options: .alwaysMapped)
        var pos = 0
        guard let (headerID, headerSize) = MatroskaSource.header(data, &pos), headerID == 0x1A45_DFA3,
              let hs = headerSize else { throw ConvertError.badFile }
        pos += Int(hs)
        // the segment (it may be preceded by junk such as Void)
        var segmentEnd = data.count
        var foundSegment = false
        while pos < data.count, let (id, size) = MatroskaSource.header(data, &pos) {
            if id == 0x1853_8067 {
                if let s = size { segmentEnd = min(data.count, pos + Int(s)) }
                foundSegment = true
                break
            }
            guard let s = size else { break }
            pos += Int(s)
        }
        guard foundSegment else { throw ConvertError.badFile }

        // the real format description is created after the tracks are parsed
        format = try MatroskaSource.placeholderFormat()
        try parseSegment(from: pos, to: segmentEnd)
        guard trackNumber != nil else { throw ConvertError.badFile }
        guard !packets.isEmpty else { throw ConvertError.badFile }
        packets.sort { $0.offset < $1.offset }
        audioPackets.sort { $0.pts < $1.pts }
    }

    private static func placeholderFormat() throws -> CMVideoFormatDescription {
        var fd: CMVideoFormatDescription?
        CMVideoFormatDescriptionCreate(allocator: nil, codecType: kCMVideoCodecType_H264, width: 2, height: 2,
                                       extensions: nil, formatDescriptionOut: &fd)
        guard let f = fd else { throw ConvertError.badFile }
        return f
    }

    func next() -> CMSampleBuffer? {
        while index < packets.count {
            let p = packets[index]
            index += 1
            if let sample = makeSample(p) { return sample }
        }
        return nil
    }

    private func makeSample(_ p: Packet) -> CMSampleBuffer? {
        guard p.length > 0, p.offset + p.length <= data.count else { return nil }
        var block: CMBlockBuffer?
        guard CMBlockBufferCreateWithMemoryBlock(allocator: kCFAllocatorDefault, memoryBlock: nil, blockLength: p.length,
                                                 blockAllocator: kCFAllocatorDefault, customBlockSource: nil,
                                                 offsetToData: 0, dataLength: p.length,
                                                 flags: kCMBlockBufferAssureMemoryNowFlag,
                                                 blockBufferOut: &block) == noErr, let bb = block else { return nil }
        let copied: OSStatus = data.withUnsafeBytes { raw in
            guard let base = raw.baseAddress else { return -1 }
            return CMBlockBufferReplaceDataBytes(with: base + p.offset, blockBuffer: bb,
                                                 offsetIntoDestination: 0, dataLength: p.length)
        }
        guard copied == noErr else { return nil }
        var timing = CMSampleTimingInfo(duration: .invalid,
                                        presentationTimeStamp: CMTime(seconds: p.pts, preferredTimescale: 90_000),
                                        decodeTimeStamp: .invalid)
        var size = p.length
        var sample: CMSampleBuffer?
        guard CMSampleBufferCreateReady(allocator: kCFAllocatorDefault, dataBuffer: bb, formatDescription: format,
                                        sampleCount: 1, sampleTimingEntryCount: 1, sampleTimingArray: &timing,
                                        sampleSizeEntryCount: 1, sampleSizeArray: &size,
                                        sampleBufferOut: &sample) == noErr else { return nil }
        return sample
    }

    // MARK: EBML parsing

    /// A variable-length integer. `keepMarker` is for element IDs. A nil value means "unknown size".
    private static func vint(_ d: Data, _ pos: inout Int, keepMarker: Bool) -> (UInt64?, Int)? {
        guard pos < d.count else { return nil }
        let first = d[pos]
        guard first != 0 else { return nil }
        var len = 1
        var mask: UInt8 = 0x80
        while first & mask == 0 {
            len += 1
            mask >>= 1
        }
        guard pos + len <= d.count else { return nil }
        var value = UInt64(keepMarker ? first : first & (mask &- 1))
        var allOnes = (first & (mask &- 1)) == (mask &- 1)
        for i in 1..<len {
            let b = d[pos + i]
            if b != 0xFF { allOnes = false }
            value = value << 8 | UInt64(b)
        }
        pos += len
        if !keepMarker && allOnes { return (nil, len) }
        return (value, len)
    }

    /// An element header: ID and size (nil = unknown).
    private static func header(_ d: Data, _ pos: inout Int) -> (UInt64, UInt64?)? {
        guard let (id, idLen) = vint(d, &pos, keepMarker: true), let idValue = id, idLen <= 4 else { return nil }
        guard let (size, _) = vint(d, &pos, keepMarker: false) else { return nil }
        return (idValue, size)
    }

    private func uint(_ from: Int, _ to: Int) -> UInt64 {
        var v: UInt64 = 0
        var i = from
        while i < to && i < from + 8 {
            v = v << 8 | UInt64(data[i])
            i += 1
        }
        return v
    }

    private func float(_ from: Int, _ to: Int) -> Double? {
        let n = to - from
        if n == 4 {
            return Double(Float(bitPattern: UInt32(truncatingIfNeeded: uint(from, to))))
        } else if n == 8 {
            return Double(bitPattern: uint(from, to))
        }
        return nil
    }

    private func string(_ from: Int, _ to: Int) -> String {
        String(decoding: data[from..<to], as: UTF8.self).trimmingCharacters(in: CharacterSet(charactersIn: "\0"))
    }

    private func parseSegment(from start: Int, to end: Int) throws {
        var pos = start
        while pos < end {
            guard let (id, size) = MatroskaSource.header(data, &pos) else { break }
            let bodyStart = pos
            switch id {
            case 0x1549_A966: // Info
                let bodyEnd = size.map { min(end, bodyStart + Int($0)) } ?? end
                parseInfo(bodyStart, bodyEnd)
                pos = bodyEnd
            case 0x1654_AE6B: // Tracks
                let bodyEnd = size.map { min(end, bodyStart + Int($0)) } ?? end
                parseTracks(bodyStart, bodyEnd)
                pos = bodyEnd
            case 0x1F43_B675: // Cluster
                if let s = size {
                    let bodyEnd = min(end, bodyStart + Int(s))
                    _ = parseCluster(bodyStart, bodyEnd, unknownSize: false)
                    pos = bodyEnd
                } else {
                    pos = parseCluster(bodyStart, end, unknownSize: true)
                }
            default:
                guard let s = size else { try finish(); return }
                pos = bodyStart + Int(s)
            }
        }
        try finish()
    }

    private func finish() throws {
        guard !finished else { return }
        finished = true
        guard trackNumber != nil, !packets.isEmpty else { return }
        format = try makeFormat()
        let lastPts = packets.map { $0.pts }.max() ?? 0
        let firstPts = packets.map { $0.pts }.min() ?? 0
        var d = durationTicks.map { $0 * timecodeScale / 1e9 } ?? 0
        if d <= 0 {
            let step = packets.count > 1 ? (lastPts - firstPts) / Double(packets.count - 1) : 1.0 / 30
            d = lastPts - firstPts + step
        }
        storedDuration = d
        storedCodecName = MatroskaSource.niceName(codecID)
    }

    private func parseInfo(_ start: Int, _ end: Int) {
        var pos = start
        while pos < end, let (id, size) = MatroskaSource.header(data, &pos), let s = size {
            let e = min(end, pos + Int(s))
            switch id {
            case 0x2A_D7B1: timecodeScale = Double(uint(pos, e))
            case 0x4489: durationTicks = float(pos, e)
            default: break
            }
            pos = e
        }
        if timecodeScale <= 0 { timecodeScale = 1_000_000 }
    }

    private func parseColour(_ start: Int, _ end: Int) {
        var pos = start
        while pos < end, let (id, size) = MatroskaSource.header(data, &pos), let s = size {
            let e = min(end, pos + Int(s))
            switch id {
            case 0x55B1: colourMatrix = Int(uint(pos, e))
            case 0x55B2: colourBits = Int(uint(pos, e))
            case 0x55BA: colourTransfer = Int(uint(pos, e))
            case 0x55BB: colourPrimaries = Int(uint(pos, e))
            default: break
            }
            pos = e
        }
    }

    private func parseTracks(_ start: Int, _ end: Int) {
        var pos = start
        while pos < end, let (id, size) = MatroskaSource.header(data, &pos), let s = size {
            let e = min(end, pos + Int(s))
            if id == 0xAE { parseTrackEntry(pos, e) }
            pos = e
        }
    }

    private func parseTrackEntry(_ start: Int, _ end: Int) {
        var pos = start
        var number: UInt64?
        var type: UInt64 = 0
        var codec = ""
        var priv = Data()
        var w = 0
        var h = 0
        var rate: Double?
        var channels: UInt64?
        while pos < end, let (id, size) = MatroskaSource.header(data, &pos), let s = size {
            let e = min(end, pos + Int(s))
            switch id {
            case 0xD7: number = uint(pos, e)
            case 0x83: type = uint(pos, e)
            case 0x86: codec = string(pos, e)
            case 0x63A2: priv = data.subdata(in: pos..<e)
            case 0xB5, 0x78B5: rate = float(pos, e)
            case 0x9F: channels = uint(pos, e)
            case 0xE0:
                var p = pos
                while p < e, let (vid, vsize) = MatroskaSource.header(data, &p), let vs = vsize {
                    let ve = min(e, p + Int(vs))
                    if vid == 0xB0 { w = Int(uint(p, ve)) }
                    if vid == 0xBA { h = Int(uint(p, ve)) }
                    if vid == 0x55B0 { parseColour(p, ve) }
                    p = ve
                }
            default: break
            }
            pos = e
        }
        guard let n = number else { return }
        if type == 1, trackNumber == nil {
            trackNumber = n
            codecID = codec
            codecPrivate = priv
            width = w
            height = h
        } else if type == 2, audioTrackNumber == nil {
            audioTrackNumber = n
            audioCodecID = codec
            audioCodecPrivate = priv
            if let rate = rate, rate > 0 { audioSampleRate = rate }
            if let channels = channels { audioChannels = Int(channels) }
        }
    }

    /// Returns the position where the cluster ended.
    private func parseCluster(_ start: Int, _ end: Int, unknownSize: Bool) -> Int {
        var pos = start
        var clusterTime: UInt64 = 0
        while pos < end {
            let save = pos
            guard let (id, size) = MatroskaSource.header(data, &pos) else { return end }
            if unknownSize && MatroskaSource.topLevel.contains(id) { return save }
            guard let s = size else { return end }
            let e = min(end, pos + Int(s))
            switch id {
            case 0xE7:
                clusterTime = uint(pos, e)
            case 0xA3: // SimpleBlock
                addBlock(pos, e, clusterTime: clusterTime, groupKey: nil)
            case 0xA0: // BlockGroup
                var p = pos
                var blockRange: (Int, Int)?
                var hasReference = false
                while p < e, let (gid, gsize) = MatroskaSource.header(data, &p), let gs = gsize {
                    let ge = min(e, p + Int(gs))
                    if gid == 0xA1 { blockRange = (p, ge) }
                    if gid == 0xFB { hasReference = true }
                    p = ge
                }
                if let r = blockRange { addBlock(r.0, r.1, clusterTime: clusterTime, groupKey: !hasReference) }
            default:
                break
            }
            pos = e
        }
        return end
    }

    private func addBlock(_ start: Int, _ end: Int, clusterTime: UInt64, groupKey: Bool?) {
        var pos = start
        guard let (num, _) = MatroskaSource.vint(data, &pos, keepMarker: false), let n = num,
              pos + 3 <= end else { return }
        let rel = Int16(bitPattern: UInt16(data[pos]) << 8 | UInt16(data[pos + 1]))
        let flags = data[pos + 2]
        pos += 3
        let lacing = Int((flags >> 1) & 0x03)
        let key = groupKey ?? (flags & 0x80 != 0)
        let pts = (Double(clusterTime) + Double(rel)) * timecodeScale / 1e9
        if n == trackNumber {
            // video frames are almost never laced; such blocks are skipped
            guard lacing == 0 else { return }
            packets.append(Packet(offset: pos, length: end - pos, pts: pts, key: key))
        } else if n == audioTrackNumber {
            appendAudioFrames(from: pos, to: end, lacing: lacing, basePts: pts)
        }
    }

    private func appendAudioFrames(from start: Int, to end: Int, lacing: Int, basePts: Double) {
        var frames: [(offset: Int, length: Int)] = []
        if lacing == 0 {
            frames = [(start, end - start)]
        } else {
            var pos = start
            guard pos < end else { return }
            let count = Int(data[pos]) + 1
            pos += 1
            var sizes: [Int] = []
            switch lacing {
            case 1:
                for _ in 0..<(count - 1) {
                    var size = 0
                    while pos < end, data[pos] == 0xFF { size += 255; pos += 1 }
                    if pos < end { size += Int(data[pos]); pos += 1 }
                    sizes.append(size)
                }
            case 2:
                var previous = 0
                for _ in 0..<(count - 1) {
                    guard let (value, length) = MatroskaSource.vint(data, &pos, keepMarker: false),
                          let v = value, length > 0 else { return }
                    let bias = (1 << (7 * length - 1)) - 1
                    previous += Int(v) - bias
                    sizes.append(previous)
                }
            default:
                let each = count > 0 ? (end - pos) / count : 0
                sizes = Array(repeating: each, count: max(0, count - 1))
            }
            var off = pos
            for i in 0..<count {
                let length = i < sizes.count ? sizes[i] : (end - off)
                guard length >= 0, off + length <= end else { break }
                frames.append((off, length))
                off += length
            }
        }
        var pts = basePts
        for frame in frames {
            guard frame.length > 0 else { continue }
            audioPackets.append(AudioPacket(offset: frame.offset, length: frame.length, pts: pts))
            pts += audioFrameDuration(offset: frame.offset, length: frame.length)
        }
    }

    private func audioFrameDuration(offset: Int, length: Int) -> Double {
        if audioCodecID == "A_OPUS" {
            let more = length > 1 ? data[offset + 1] : nil
            return MatroskaSource.opusDuration(data[offset], moreByte: more)
        }
        if audioSampleRate > 0 { return 1024.0 / audioSampleRate }
        return 0.02
    }

    private static func opusDuration(_ toc: UInt8, moreByte: UInt8?) -> Double {
        let config = Int(toc) >> 3
        let code = Int(toc) & 3
        let frames: Int
        switch code {
        case 0: frames = 1
        case 1, 2: frames = 2
        default: frames = moreByte.map { Int($0) & 0x3F } ?? 1
        }
        let perFrame: Double
        if config < 12 { perFrame = [10.0, 20, 40, 60][config & 3] }
        else if config < 16 { perFrame = [10.0, 20, 10, 20][config & 3] }
        else { perFrame = [2.5, 5, 10, 20][config & 3] }
        return perFrame * Double(max(1, frames)) / 1000.0
    }

    private static func aacConfig(_ asc: Data) -> (rate: Double, channels: Int)? {
        let rates: [Double] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350]
        var reader = BitReader(bytes: [UInt8](asc))
        var objectType = reader.read(5)
        if objectType == 31 { objectType = 32 + reader.read(6) }
        let index = reader.read(4)
        let rate: Double
        if index == 15 { rate = Double(reader.read(24)) }
        else {
            guard index < rates.count else { return nil }
            rate = rates[index]
        }
        let channels = reader.read(4)
        guard rate > 0 else { return nil }
        return (rate, channels == 0 ? 2 : channels)
    }

    // MARK: Format description

    private static func niceName(_ codecID: String) -> String {
        switch codecID {
        case "V_VP9": return "VP9"
        case "V_VP8": return "VP8"
        case "V_AV1": return "AV1"
        case "V_MPEG4/ISO/AVC": return "H.264"
        case "V_MPEGH/ISO/HEVC": return "HEVC"
        default: return codecID.hasPrefix("V_") ? String(codecID.dropFirst(2)) : codecID
        }
    }

    private func makeFormat() throws -> CMVideoFormatDescription {
        var codecType: CMVideoCodecType
        var atoms: [String: Any] = [:]
        var w = width
        var h = height
        switch codecID {
        case "V_VP9":
            codecType = VideoConverter.vp9
            guard let info = vp9Info() else { throw ConvertError.badFile }
            atoms["vpcC"] = info.vpcC
            if w == 0 || h == 0 { w = info.width; h = info.height }
        case "V_AV1":
            codecType = VideoConverter.av1
            guard !codecPrivate.isEmpty else { throw ConvertError.badFile }
            atoms["av1C"] = codecPrivate
            if !VTIsHardwareDecodeSupported(codecType) { throw ConvertError.decoder("AV1") }
        case "V_MPEG4/ISO/AVC":
            codecType = kCMVideoCodecType_H264
            atoms["avcC"] = codecPrivate
        case "V_MPEGH/ISO/HEVC":
            codecType = kCMVideoCodecType_HEVC
            atoms["hvcC"] = codecPrivate
        default:
            throw ConvertError.unsupportedCodec(MatroskaSource.niceName(codecID))
        }
        guard w > 0, h > 0 else { throw ConvertError.badFile }
        let ext: [String: Any] = [kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms as String: atoms]
        var fd: CMVideoFormatDescription?
        let st = CMVideoFormatDescriptionCreate(allocator: nil, codecType: codecType, width: Int32(w), height: Int32(h),
                                                extensions: ext as CFDictionary, formatDescriptionOut: &fd)
        guard st == noErr, let f = fd else { throw ConvertError.badFile }
        return f
    }

    /// VP9 profile, bit depth and size, from the header of the first key frame.
    private func vp9Info() -> (vpcC: Data, width: Int, height: Int, bitDepth: Int)? {
        for p in packets.prefix(60) where p.key {
            let bytes = [UInt8](data[p.offset..<min(p.offset + 32, p.offset + p.length)])
            var r = BitReader(bytes: bytes)
            guard r.read(2) == 2 else { continue }
            let low = r.read(1)
            let high = r.read(1)
            let profile = high << 1 | low
            if profile == 3 { _ = r.read(1) }
            if r.read(1) == 1 { continue }          // show_existing_frame
            let frameType = r.read(1)
            _ = r.read(2)                           // show_frame, error_resilient_mode
            guard frameType == 0, r.read(24) == 0x49_8342 else { continue }
            var bitDepth = 8
            if profile >= 2 { bitDepth = r.read(1) == 1 ? 12 : 10 }
            let colorSpace = r.read(3)
            var fullRange = 0
            var ssx = 1
            var ssy = 1
            if colorSpace != 7 {
                fullRange = r.read(1)
                if profile == 1 || profile == 3 {
                    ssx = r.read(1)
                    ssy = r.read(1)
                    _ = r.read(1)
                }
            } else {
                fullRange = 1
                ssx = 0
                ssy = 0
                if profile == 1 || profile == 3 { _ = r.read(1) }
            }
            let w = r.read(16) + 1
            let h = r.read(16) + 1
            let chroma: Int = (ssx == 1 && ssy == 1) ? 1 : (ssx == 1 ? 2 : 3)
            let colors: (UInt8, UInt8, UInt8)
            switch colorSpace {
            case 1, 3: colors = (6, 6, 6)       // BT.601 / SMPTE 170M
            case 4: colors = (7, 7, 7)          // SMPTE 240M
            case 5: colors = (9, 14, 9)         // BT.2020
            case 7: colors = (1, 13, 0)         // sRGB
            default: colors = (1, 1, 1)         // BT.709
            }
            let pixels = w * h
            let level: UInt8 = pixels <= 1280 * 720 ? 31 : (pixels <= 2048 * 1088 ? 41 : (pixels <= 4096 * 2176 ? 51 : 61))
            let vpcC = Data([1, 0, 0, 0,
                             UInt8(profile), level,
                             UInt8(bitDepth << 4 | chroma << 1 | fullRange),
                             colors.0, colors.1, colors.2,
                             0, 0])
            return (vpcC, w, h, bitDepth)
        }
        return nil
    }
}

private struct BitReader {
    let bytes: [UInt8]
    var pos = 0

    mutating func read(_ n: Int) -> Int {
        var v = 0
        for _ in 0..<n {
            let byte = pos >> 3
            let bit = byte < bytes.count ? Int((bytes[byte] >> (7 - UInt8(pos & 7))) & 1) : 0
            v = v << 1 | bit
            pos += 1
        }
        return v
    }
}

private enum AudioAttach {
    case none
    case attached
    case dropped
}

private final class DecodeFeed {
    let base: UnsafeRawPointer
    let packets: [(offset: Int, length: Int)]
    let framesHint: UInt32
    var index = 0

    init(base: UnsafeRawPointer, packets: [(offset: Int, length: Int)], framesHint: UInt32) {
        self.base = base
        self.packets = packets
        self.framesHint = framesHint
    }
}

private let audioInputProc: AudioConverterComplexInputDataProc = { _, ioNumberDataPackets, ioData, outDescription, inUserData in
    let feed = Unmanaged<DecodeFeed>.fromOpaque(inUserData!).takeUnretainedValue()
    guard feed.index < feed.packets.count else {
        ioNumberDataPackets.pointee = 0
        return noErr
    }
    let packet = feed.packets[feed.index]
    feed.index += 1
    ioNumberDataPackets.pointee = 1
    let list = UnsafeMutableAudioBufferListPointer(ioData)
    list[0].mData = UnsafeMutableRawPointer(mutating: feed.base + packet.offset)
    list[0].mDataByteSize = UInt32(packet.length)
    list[0].mNumberChannels = 1
    outDescription?.pointee?.pointee = AudioStreamPacketDescription(mStartOffset: 0,
                                                                    mVariableFramesInPacket: feed.framesHint,
                                                                    mDataByteSize: UInt32(packet.length))
    return noErr
}

extension VideoConverter {
    private static let movAudioSubtypes: Set<FourCharCode> = {
        let ids: [AudioFormatID] = [kAudioFormatMPEG4AAC, kAudioFormatAppleLossless, kAudioFormatAC3,
                                    kAudioFormatEnhancedAC3, kAudioFormatLinearPCM, kAudioFormatMPEGLayer3]
        var set = Set(ids)
        set.insert(0x6D7034_61)
        return set
    }()

    private static func attachAudio(source: URL, video: URL, videoStart: Double,
                                    token: CancelToken) async throws -> AudioAttach {
        var audioFile: URL?
        do {
            audioFile = try await makeAudioFile(source: source, videoStart: videoStart, token: token)
        } catch let error as OptimizeError {
            if case .cancelled = error { throw error }
            return .dropped
        } catch {
            return .dropped
        }
        guard let audioFile = audioFile else { return .none }
        defer { try? FileManager.default.removeItem(at: audioFile) }
        if token.isCancelled { throw OptimizeError.cancelled }
        let muxed = video.deletingLastPathComponent().appendingPathComponent("\(UUID().uuidString).mov")
        var result: AudioAttach = .dropped
        do {
            try await VideoMuxer.addAudio(video: video, audio: audioFile, dest: muxed)
            result = .attached
        } catch {
            result = .dropped
        }
        if token.isCancelled {
            try? FileManager.default.removeItem(at: muxed)
            throw OptimizeError.cancelled
        }
        if result == .attached {
            do { _ = try FileManager.default.replaceItemAt(video, withItemAt: muxed) }
            catch { try? FileManager.default.removeItem(at: muxed); result = .dropped }
        }
        return result
    }

    private static func makeAudioFile(source: URL, videoStart: Double, token: CancelToken) async throws -> URL? {
        let asset = AVURLAsset(url: source)
        if let track = try? await asset.loadTracks(withMediaType: .audio).first {
            return try await extractReadable(asset: asset, track: track, videoStart: videoStart, token: token)
        }
        if let matroska = try? MatroskaSource(url: source) {
            if matroska.hasAudio {
                return try await extractMatroska(matroska, videoStart: videoStart, token: token)
            }
            return nil
        }
        return nil
    }

    private static func extractReadable(asset: AVAsset, track: AVAssetTrack, videoStart: Double,
                                        token: CancelToken) async throws -> URL {
        guard let hint = (try? await track.load(.formatDescriptions))?.first else { throw ConvertError.audio }
        let compatible = movAudioSubtypes.contains(CMFormatDescriptionGetMediaSubType(hint))
        let reader = try AVAssetReader(asset: asset)
        let output: AVAssetReaderTrackOutput
        let input: AVAssetWriterInput
        if compatible {
            output = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
            input = AVAssetWriterInput(mediaType: .audio, outputSettings: nil, sourceFormatHint: hint)
        } else {
            output = AVAssetReaderTrackOutput(track: track, outputSettings: [
                AVFormatIDKey: kAudioFormatLinearPCM,
                AVLinearPCMBitDepthKey: 16,
                AVLinearPCMIsFloatKey: false,
                AVLinearPCMIsBigEndianKey: false,
                AVLinearPCMIsNonInterleaved: false,
                AVSampleRateKey: 48_000,
                AVNumberOfChannelsKey: 2,
            ])
            input = AVAssetWriterInput(mediaType: .audio, outputSettings: [
                AVFormatIDKey: kAudioFormatMPEG4AAC,
                AVSampleRateKey: 48_000,
                AVNumberOfChannelsKey: 2,
                AVEncoderBitRateKey: 176_000,
            ])
        }
        output.alwaysCopiesSampleData = false
        input.expectsMediaDataInRealTime = false
        guard reader.canAdd(output) else { throw ConvertError.audio }
        reader.add(output)
        let writer = try AVAssetWriter(outputURL: audioURL(), fileType: .mov)
        guard writer.canAdd(input) else { throw ConvertError.audio }
        writer.add(input)
        guard reader.startReading(), writer.startWriting() else { throw ConvertError.audio }
        writer.startSession(atSourceTime: CMTime(seconds: videoStart, preferredTimescale: 90_000))
        func cancelReading() {
            reader.cancelReading()
            writer.cancelWriting()
            try? FileManager.default.removeItem(at: writer.outputURL)
        }
        while true {
            if token.isCancelled { cancelReading(); throw OptimizeError.cancelled }
            guard let sample = output.copyNextSampleBuffer() else { break }
            let end = CMTimeGetSeconds(CMTimeAdd(CMSampleBufferGetPresentationTimeStamp(sample),
                                                 CMSampleBufferGetDuration(sample)))
            if end.isFinite, end <= videoStart { continue }
            while !input.isReadyForMoreMediaData {
                if token.isCancelled { cancelReading(); throw OptimizeError.cancelled }
                try? await Task.sleep(nanoseconds: 2_000_000)
            }
            guard input.append(sample) else { break }
        }
        input.markAsFinished()
        await writer.finishWriting()
        guard writer.status == .completed else {
            try? FileManager.default.removeItem(at: writer.outputURL)
            throw ConvertError.audio
        }
        return writer.outputURL
    }

    private static func extractMatroska(_ source: MatroskaSource, videoStart: Double,
                                        token: CancelToken) async throws -> URL {
        guard let track = source.audioTrack else { throw ConvertError.audio }
        let dest = audioURL()
        try? FileManager.default.removeItem(at: dest)
        var asbd = lpcmDescription()
        var format: CMAudioFormatDescription?
        guard CMAudioFormatDescriptionCreate(allocator: nil, asbd: &asbd, layoutSize: 0, layout: nil,
                                             magicCookieSize: 0, magicCookie: nil, extensions: nil,
                                             formatDescriptionOut: &format) == noErr, format != nil else {
            throw ConvertError.audio
        }
        let writer = try AVAssetWriter(outputURL: dest, fileType: .mov)
        let input = AVAssetWriterInput(mediaType: .audio, outputSettings: [
            AVFormatIDKey: kAudioFormatMPEG4AAC,
            AVSampleRateKey: 48_000,
            AVNumberOfChannelsKey: 2,
            AVEncoderBitRateKey: 176_000,
        ], sourceFormatHint: format)
        input.expectsMediaDataInRealTime = false
        guard writer.canAdd(input) else { throw ConvertError.audio }
        writer.add(input)
        guard writer.startWriting() else { throw ConvertError.audio }
        writer.startSession(atSourceTime: .zero)

        var converter: AudioConverterRef?
        var src = track.streamDescription(rate: track.sampleRate, channels: track.channels)
        var dst = asbd
        guard AudioConverterNew(&src, &dst, &converter) == noErr, let decoder = converter else {
            writer.cancelWriting(); try? FileManager.default.removeItem(at: dest)
            throw ConvertError.audio
        }
        defer { AudioConverterDispose(decoder) }
        if !track.codecPrivate.isEmpty {
            track.codecPrivate.withUnsafeBytes { raw in
                _ = AudioConverterSetProperty(decoder, kAudioConverterDecompressionMagicCookie,
                                              UInt32(track.codecPrivate.count), raw.baseAddress!)
            }
        }

        let bytesPerFrame = 4
        var pcm = [Int16](repeating: 0, count: 8192 * 2)
        do {
            _ = try source.withAudioBytes { base -> Int in
                var feedPackets: [(offset: Int, length: Int)] = []
                for packet in source.audioPackets {
                    guard packet.offset + packet.length <= source.byteCount else { continue }
                    feedPackets.append((packet.offset, packet.length))
                }
                let feed = DecodeFeed(base: base, packets: feedPackets, framesHint: track.framesPerPacket)
                var discard = track.preSkip
                var first = true
                var written = 0
                var basePts = 0.0
                let list = AudioBufferList.allocate(maximumBuffers: 1)
                defer { free(list.unsafeMutablePointer) }
                while true {
                    if token.isCancelled { throw OptimizeError.cancelled }
                    list[0] = AudioBuffer(mNumberChannels: 2, mDataByteSize: UInt32(pcm.count * 2),
                                          mData: nil)
                    var count = UInt32(8192)
                    let status: OSStatus = pcm.withUnsafeMutableBytes { raw in
                        list[0].mData = raw.baseAddress
                        return AudioConverterFillComplexBuffer(decoder, audioInputProc,
                                                               Unmanaged.passUnretained(feed).toOpaque(),
                                                               &count, list.unsafeMutablePointer, nil)
                    }
                    if status != noErr { throw ConvertError.audio }
                    var frames = Int(count)
                    if frames == 0 { break }
                    var offset = 0
                    if first {
                        first = false
                        basePts = source.audioPackets.first.map { $0.pts - videoStart } ?? -videoStart
                    }
                    if discard > 0 {
                        let drop = min(discard, frames)
                        discard -= drop
                        frames -= drop
                        offset = drop
                    }
                    if frames <= 0 { continue }
                    let time = basePts + Double(written) / 48_000
                    if time + Double(frames) / 48_000 <= 0 { written += frames; continue }
                    while !input.isReadyForMoreMediaData {
                        if token.isCancelled { throw OptimizeError.cancelled }
                        usleep(2000)
                    }
                    let byteLength = frames * bytesPerFrame
                    guard let sample = makePCMSample(pcm: pcm, byteOffset: offset * bytesPerFrame, byteLength: byteLength,
                                                     frames: frames, time: CMTime(seconds: max(0, time), preferredTimescale: 90_000),
                                                     format: format!) else {
                        throw ConvertError.audio
                    }
                    if !input.append(sample) { throw ConvertError.audio }
                    written += frames
                }
                return written
            }
        } catch {
            writer.cancelWriting()
            try? FileManager.default.removeItem(at: dest)
            throw error
        }
        input.markAsFinished()
        await writer.finishWriting()
        guard writer.status == .completed else {
            try? FileManager.default.removeItem(at: dest)
            throw ConvertError.audio
        }
        return dest
    }

    private static func makePCMSample(pcm: [Int16], byteOffset: Int, byteLength: Int, frames: Int,
                                      time: CMTime, format: CMAudioFormatDescription) -> CMSampleBuffer? {
        var block: CMBlockBuffer?
        guard CMBlockBufferCreateWithMemoryBlock(allocator: kCFAllocatorDefault, memoryBlock: nil,
                                                 blockLength: byteLength, blockAllocator: kCFAllocatorDefault,
                                                 customBlockSource: nil, offsetToData: 0, dataLength: byteLength,
                                                 flags: kCMBlockBufferAssureMemoryNowFlag,
                                                 blockBufferOut: &block) == noErr, let buffer = block else { return nil }
        let copied: OSStatus = pcm.withUnsafeBytes { raw in
            guard let base = raw.baseAddress else { return -1 }
            return CMBlockBufferReplaceDataBytes(with: base + byteOffset, blockBuffer: buffer,
                                                 offsetIntoDestination: 0, dataLength: byteLength)
        }
        guard copied == noErr else { return nil }
        var timing = CMSampleTimingInfo(duration: CMTime(value: 1, timescale: 48_000),
                                        presentationTimeStamp: time, decodeTimeStamp: .invalid)
        var size = 4
        var sample: CMSampleBuffer?
        guard CMSampleBufferCreateReady(allocator: kCFAllocatorDefault, dataBuffer: buffer,
                                        formatDescription: format, sampleCount: frames,
                                        sampleTimingEntryCount: 1, sampleTimingArray: &timing,
                                        sampleSizeEntryCount: 1, sampleSizeArray: &size,
                                        sampleBufferOut: &sample) == noErr else { return nil }
        return sample
    }

    private static func lpcmDescription() -> AudioStreamBasicDescription {
        var d = AudioStreamBasicDescription()
        d.mSampleRate = 48_000
        d.mFormatID = kAudioFormatLinearPCM
        d.mFormatFlags = kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked | kAudioFormatFlagsNativeEndian
        d.mChannelsPerFrame = 2
        d.mBitsPerChannel = 16
        d.mFramesPerPacket = 1
        d.mBytesPerFrame = 4
        d.mBytesPerPacket = 4
        return d
    }

    private static func audioURL() -> URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("\(UUID().uuidString).mov")
    }
}

// MARK: - Joining a separate sound track

enum VideoMuxer {
    /// Writes `video` with the sound from `audio` into `dest` (.mov) without re-encoding.
    static func addAudio(video: URL, audio: URL, dest: URL) async throws {
        let videoAsset = AVURLAsset(url: video)
        let audioAsset = AVURLAsset(url: audio)
        guard let videoTrack = try await videoAsset.loadTracks(withMediaType: .video).first,
              let audioTrack = try await audioAsset.loadTracks(withMediaType: .audio).first else {
            throw ConvertError.badFile
        }
        let videoDuration = try await videoAsset.load(.duration)
        let audioDuration = try await audioAsset.load(.duration)
        let composition = AVMutableComposition()
        guard let v = composition.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid),
              let a = composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid) else {
            throw ConvertError.encoder
        }
        try v.insertTimeRange(CMTimeRange(start: .zero, duration: videoDuration), of: videoTrack, at: .zero)
        v.preferredTransform = try await videoTrack.load(.preferredTransform)
        try a.insertTimeRange(CMTimeRange(start: .zero, duration: CMTimeMinimum(videoDuration, audioDuration)),
                              of: audioTrack, at: .zero)

        try? FileManager.default.removeItem(at: dest)
        guard let export = AVAssetExportSession(asset: composition, presetName: AVAssetExportPresetPassthrough) else {
            throw ConvertError.encoder
        }
        export.outputURL = dest
        export.outputFileType = .mov
        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
            export.exportAsynchronously { c.resume() }
        }
        guard export.status == .completed else {
            try? FileManager.default.removeItem(at: dest)
            throw ConvertError.encoder
        }
    }
}
