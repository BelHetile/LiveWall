import AVFoundation
import CoreMedia
import VideoToolbox

enum ConvertError: LocalizedError {
    case unsupportedCodec(String)
    case decoder(String)
    case encoder
    case badFile

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
        }
    }
}

/// Источник сжатых кадров (в порядке декодирования) для VideoToolbox.
private protocol PacketSource: AnyObject {
    var format: CMVideoFormatDescription { get }
    var duration: Double { get }
    var transform: CGAffineTransform { get }
    var codecName: String { get }
    func next() -> CMSampleBuffer?
}

/// Переводит видео, которое macOS не умеет проигрывать (WebM, MKV, VP9/AV1 в MP4), в обычный HEVC.
enum VideoConverter {
    static let vp9: CMVideoCodecType = 0x7670_3039   // 'vp09'
    static let av1: CMVideoCodecType = 0x6176_3031   // 'av01'

    /// Проигрывает ли файл сама macOS (AVFoundation).
    static func canPlayNatively(_ url: URL) async -> Bool {
        let ext = url.pathExtension.lowercased()
        if ["webm", "mkv"].contains(ext) { return false }
        let asset = AVURLAsset(url: url)
        guard let track = try? await asset.loadTracks(withMediaType: .video).first else { return false }
        let playable = (try? await asset.load(.isPlayable)) ?? false
        let decodable = (try? await track.load(.isDecodable)) ?? false
        return playable && decodable
    }

    static func convert(source: URL, dest: URL, fpsLimit: Double, target: CGSize,
                        token: CancelToken, progress: @escaping (Double) -> Void) async throws {
        VTRegisterSupplementalVideoDecoderIfAvailable(vp9)
        do {
            try await run(source: source, dest: dest, fpsLimit: fpsLimit, target: target,
                          codec: .hevc, token: token, progress: progress)
        } catch ConvertError.encoder {
            // без кодировщика HEVC (редко) — H.264
            try await run(source: source, dest: dest, fpsLimit: fpsLimit, target: target,
                          codec: .h264, token: token, progress: progress)
        }
    }

    private static func makeSource(_ url: URL) async throws -> PacketSource {
        let ext = url.pathExtension.lowercased()
        if ["webm", "mkv"].contains(ext) { return try MatroskaSource(url: url) }
        if let source = try? await AssetPacketSource.make(url: url) { return source }
        // расширение врёт — вдруг внутри Matroska
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
                            token: CancelToken, progress: @escaping (Double) -> Void) async throws {
        let source = try await makeSource(url)
        let format = source.format
        let dims = CMVideoFormatDescriptionGetDimensions(format)
        guard dims.width > 0, dims.height > 0 else { throw ConvertError.badFile }

        // декодер
        var session: VTDecompressionSession?
        let attrs: [String: Any] = [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
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

        // размер: покрыть экран, но не увеличивать
        let natural = CGSize(width: CGFloat(dims.width), height: CGFloat(dims.height))
        let shown = CGRect(origin: .zero, size: natural).applying(source.transform)
        let scale = min(1, max(target.width / max(abs(shown.width), 1), target.height / max(abs(shown.height), 1)))
        let outW = max(2, Int((natural.width * scale / 2).rounded()) * 2)
        let outH = max(2, Int((natural.height * scale / 2).rounded()) * 2)
        let bitsPerPixel = codec == .hevc ? 0.07 : 0.12
        let bitrate = max(2_000_000, Int(Double(outW * outH) * fpsLimit * bitsPerPixel))

        try? FileManager.default.removeItem(at: dest)
        guard let writer = try? AVAssetWriter(outputURL: dest, fileType: .mov) else { throw ConvertError.encoder }
        let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: codec,
            AVVideoWidthKey: outW,
            AVVideoHeightKey: outH,
            AVVideoScalingModeKey: AVVideoScalingModeResizeAspectFill,
            AVVideoCompressionPropertiesKey: [
                AVVideoAverageBitRateKey: bitrate,
                AVVideoExpectedSourceFrameRateKey: Int(fpsLimit.rounded()),
                AVVideoMaxKeyFrameIntervalKey: Int(fpsLimit.rounded()) * 2,
            ] as [String: Any],
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

        // кадры декодер отдаёт в порядке декодирования — сортируем по времени показа небольшим буфером
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
                // ни одного кадра после десятков попыток — декодер не справляется
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
    }
}

// MARK: - MP4 / MOV с кодеком, который AVFoundation сама не декодирует

private final class AssetPacketSource: PacketSource {
    let reader: AVAssetReader
    let output: AVAssetReaderTrackOutput
    let format: CMVideoFormatDescription
    let duration: Double
    let transform: CGAffineTransform
    let codecName: String

    private init(reader: AVAssetReader, output: AVAssetReaderTrackOutput, format: CMVideoFormatDescription,
                 duration: Double, transform: CGAffineTransform) {
        self.reader = reader
        self.output = output
        self.format = format
        self.duration = duration
        self.transform = transform
        codecName = VideoOptimizer.codecName(CMFormatDescriptionGetMediaSubType(format))
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

    private(set) var format: CMVideoFormatDescription
    private var storedDuration: Double = 0
    private var storedCodecName = ""
    private var finished = false
    var duration: Double { storedDuration }
    var codecName: String { storedCodecName }
    let transform = CGAffineTransform.identity

    // элементы верхнего уровня сегмента: на них заканчивается кластер неизвестной длины
    private static let topLevel: Set<UInt64> = [0x1F43_B675, 0x1C53_BB6B, 0x1254_C367, 0x1549_A966,
                                                0x1654_AE6B, 0x114D_9B74, 0x1043_A770, 0x1941_A469]

    init(url: URL) throws {
        data = try Data(contentsOf: url, options: .alwaysMapped)
        var pos = 0
        guard let (headerID, headerSize) = MatroskaSource.header(data, &pos), headerID == 0x1A45_DFA3,
              let hs = headerSize else { throw ConvertError.badFile }
        pos += Int(hs)
        // сегмент (перед ним может быть мусор вроде Void)
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

        // настоящее описание формата появится после разбора дорожек
        format = try MatroskaSource.placeholderFormat()
        try parseSegment(from: pos, to: segmentEnd)
        guard trackNumber != nil else { throw ConvertError.badFile }
        guard !packets.isEmpty else { throw ConvertError.badFile }
        packets.sort { $0.offset < $1.offset }
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

    // MARK: Разбор EBML

    /// Число переменной длины. `keepMarker` — для идентификаторов. nil в значении — «неизвестный размер».
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

    /// Заголовок элемента: идентификатор и размер (nil — неизвестный).
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

    private func parseTracks(_ start: Int, _ end: Int) {
        var pos = start
        while pos < end, let (id, size) = MatroskaSource.header(data, &pos), let s = size {
            let e = min(end, pos + Int(s))
            if id == 0xAE, trackNumber == nil { parseTrackEntry(pos, e) }
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
        while pos < end, let (id, size) = MatroskaSource.header(data, &pos), let s = size {
            let e = min(end, pos + Int(s))
            switch id {
            case 0xD7: number = uint(pos, e)
            case 0x83: type = uint(pos, e)
            case 0x86: codec = string(pos, e)
            case 0x63A2: priv = data.subdata(in: pos..<e)
            case 0xE0:
                var p = pos
                while p < e, let (vid, vsize) = MatroskaSource.header(data, &p), let vs = vsize {
                    let ve = min(e, p + Int(vs))
                    if vid == 0xB0 { w = Int(uint(p, ve)) }
                    if vid == 0xBA { h = Int(uint(p, ve)) }
                    p = ve
                }
            default: break
            }
            pos = e
        }
        guard type == 1, let n = number else { return }
        trackNumber = n
        codecID = codec
        codecPrivate = priv
        width = w
        height = h
    }

    /// Возвращает позицию, где закончился кластер.
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
        guard let track = trackNumber else { return }
        var pos = start
        guard let (num, _) = MatroskaSource.vint(data, &pos, keepMarker: false), let n = num, n == track,
              pos + 3 <= end else { return }
        let rel = Int16(bitPattern: UInt16(data[pos]) << 8 | UInt16(data[pos + 1]))
        let flags = data[pos + 2]
        pos += 3
        // кадры видео почти никогда не «упакованы» — такие блоки пропускаем
        guard (flags >> 1) & 0x03 == 0 else { return }
        let key = groupKey ?? (flags & 0x80 != 0)
        let pts = (Double(clusterTime) + Double(rel)) * timecodeScale / 1e9
        packets.append(Packet(offset: pos, length: end - pos, pts: pts, key: key))
    }

    // MARK: Описание формата

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

    /// Профиль, глубина цвета и размер VP9 — из заголовка первого ключевого кадра.
    private func vp9Info() -> (vpcC: Data, width: Int, height: Int)? {
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
            return (vpcC, w, h)
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
