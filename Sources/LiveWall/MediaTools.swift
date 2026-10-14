import AppKit
import CryptoKit
import VideoToolbox

/// yt-dlp and Deno: downloading videos from YouTube and similar sites.
/// Both are free software; LiveWall uses them from Homebrew or downloads them from their official GitHub releases.
enum MediaTools {
    static let dir: URL = {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let d = base.appendingPathComponent("LiveWall/Tools", isDirectory: true)
        try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        return d
    }()

    static func find(_ name: String) -> URL? {
        let candidates = [dir.appendingPathComponent(name).path, "/opt/homebrew/bin/\(name)", "/usr/local/bin/\(name)"]
        return candidates.first(where: { FileManager.default.isExecutableFile(atPath: $0) }).map { URL(fileURLWithPath: $0) }
    }

    static var ytdlp: URL? { find("yt-dlp") }
    static var deno: URL? { find("deno") }
    static var hasYtdlp: Bool { ytdlp != nil }
    static var hasDeno: Bool { deno != nil }
    static var installed: Bool { hasYtdlp && hasDeno }
    /// Downloaded by LiveWall itself (so they can be updated), not from Homebrew.
    static var ownCopy: Bool { FileManager.default.fileExists(atPath: dir.appendingPathComponent("yt-dlp").path) }

    private static let videoSites = ["youtube.com", "youtu.be", "youtube-nocookie.com", "vimeo.com", "tiktok.com",
                                     "twitch.tv", "dailymotion.com", "bilibili.com", "b23.tv", "x.com", "twitter.com",
                                     "instagram.com", "facebook.com", "fb.watch", "reddit.com", "redd.it", "streamable.com",
                                     "vk.com", "vkvideo.ru", "rutube.ru", "pinterest.com", "pin.it"]

    private static func matches(_ host: String, _ sites: [String]) -> Bool {
        sites.contains { host == $0 || host.hasSuffix("." + $0) }
    }

    /// Sites that don't serve videos as files: go straight to yt-dlp.
    static func isVideoSite(_ url: URL) -> Bool {
        guard let host = url.host?.lowercased() else { return false }
        return matches(host, videoSites)
    }

    static func needsDeno(_ url: URL) -> Bool {
        guard let host = url.host?.lowercased() else { return false }
        return matches(host, ["youtube.com", "youtu.be", "youtube-nocookie.com"])
    }

    enum ToolError: LocalizedError {
        case download(String)
        case failed(String)

        var errorDescription: String? {
            switch self {
            case .download(let text): return LF("Could not download the tools: %@", text)
            case .failed(let text): return text
            }
        }

        var isFormatUnavailable: Bool {
            guard case .failed(let text) = self else { return false }
            return text.localizedCaseInsensitiveContains("format is not available")
                || text.localizedCaseInsensitiveContains("no video formats found")
        }
    }

    /// Asks the user for consent to download the tools.
    static func askToInstall() -> Bool {
        let alert = NSAlert()
        alert.messageText = L("Download video tools?")
        alert.informativeText = L("To add videos from YouTube and similar sites, LiveWall uses two free open-source tools: yt-dlp and Deno (about 100 MB, from their official GitHub releases). They are stored in LiveWall's folder and can be updated in Settings.")
        alert.addButton(withTitle: L("Download"))
        alert.addButton(withTitle: L("Cancel"))
        NSApp.activate(ignoringOtherApps: true)
        return alert.runModal() == .alertFirstButtonReturn
    }

    /// Downloads (or updates) yt-dlp and Deno into LiveWall's folder.
    static func install(deno wantsDeno: Bool, progress: @escaping (Double) -> Void) async throws {
        let ytTag = try await resolveTag(owner: "yt-dlp", repo: "yt-dlp")
        let ytBase = "https://github.com/yt-dlp/yt-dlp/releases/download/\(ytTag)"
        let ytSums = try await fetchCheckSums(URL(string: "\(ytBase)/SHA2-256SUMS")!)

        let ytShare = wantsDeno ? 0.4 : 0.9
        let yt = try await download(URL(string: "\(ytBase)/yt-dlp_macos")!) { progress($0 * ytShare) }
        try verify(yt, sums: ytSums, name: "yt-dlp_macos")
        let ytDest = dir.appendingPathComponent("yt-dlp")
        _ = try FileManager.default.replaceItemAt(ytDest, withItemAt: yt, backupItemName: nil, options: [])
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: ytDest.path)

        if wantsDeno {
            #if arch(arm64)
            let denoName = "deno-aarch64-apple-darwin.zip"
            #else
            let denoName = "deno-x86_64-apple-darwin.zip"
            #endif
            let denoTag = try await resolveTag(owner: "denoland", repo: "deno")
            let denoBase = "https://github.com/denoland/deno/releases/download/\(denoTag)"
            let denoSums = try await fetchCheckSums(URL(string: "\(denoBase)/\(denoName).sha256sum")!)

            let zip = try await download(URL(string: "\(denoBase)/\(denoName)")!) { progress(0.4 + $0 * 0.55) }
            defer { try? FileManager.default.removeItem(at: zip) }
            try verify(zip, sums: denoSums, name: denoName)
            let unpack = dir.appendingPathComponent("deno-unpack", isDirectory: true)
            try? FileManager.default.removeItem(at: unpack)
            let status = try runQuiet(URL(fileURLWithPath: "/usr/bin/ditto"), ["-x", "-k", zip.path, unpack.path])
            guard status == 0 else { throw ToolError.download("unzip") }
            let denoDest = dir.appendingPathComponent("deno")
            _ = try FileManager.default.replaceItemAt(denoDest, withItemAt: unpack.appendingPathComponent("deno"), backupItemName: nil, options: [])
            try? FileManager.default.removeItem(at: unpack)
            try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: denoDest.path)
        }
        progress(1)
    }

    private static func resolveTag(owner: String, repo: String) async throws -> String {
        var request = URLRequest(url: URL(string: "https://github.com/\(owner)/\(repo)/releases/latest")!)
        request.httpMethod = "HEAD"
        let (_, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode),
              let parts = response.url?.pathComponents, parts.count >= 2, parts[parts.count - 2] == "tag",
              let tag = parts.last, !tag.isEmpty else {
            throw ToolError.failed(L("Could not determine the latest version."))
        }
        return tag
    }

    private static func fetchCheckSums(_ url: URL) async throws -> String {
        let (data, response) = try await URLSession.shared.data(from: url)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode),
              let text = String(data: data, encoding: .utf8) else {
            throw ToolError.failed(L("The download could not be verified: no checksum was published for it."))
        }
        return text
    }

    private static func checksumFor(_ name: String, in text: String) -> String? {
        for line in text.split(whereSeparator: { $0 == "\n" || $0 == "\r" }) {
            let fields = line.split(separator: " ", omittingEmptySubsequences: true)
            guard fields.count >= 2, fields.dropFirst().joined(separator: " ") == name else { continue }
            return fields[0].lowercased()
        }
        return nil
    }

    private static func sha256(of file: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: file)
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try handle.read(upToCount: 1 << 20), !chunk.isEmpty {
            hasher.update(data: chunk)
        }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }

    private static func verify(_ file: URL, sums: String, name: String) throws {
        guard let expected = checksumFor(name, in: sums) else {
            try? FileManager.default.removeItem(at: file)
            throw ToolError.failed(L("The download could not be verified: no checksum was published for it."))
        }
        guard let actual = try? sha256(of: file), actual == expected else {
            try? FileManager.default.removeItem(at: file)
            throw ToolError.failed(L("The downloaded file failed its integrity check."))
        }
    }

    private static func runQuiet(_ exe: URL, _ args: [String]) throws -> Int32 {
        let p = Process()
        p.executableURL = exe
        p.arguments = args
        p.standardOutput = FileHandle.nullDevice
        p.standardError = FileHandle.nullDevice
        try p.run()
        p.waitUntilExit()
        return p.terminationStatus
    }

    private final class DownloadState {
        var observation: NSKeyValueObservation?
    }

    private static func download(_ url: URL, progress: @escaping (Double) -> Void) async throws -> URL {
        let state = DownloadState()
        return try await withCheckedThrowingContinuation { (c: CheckedContinuation<URL, Error>) in
            let task = URLSession.shared.downloadTask(with: url) { tmp, response, error in
                state.observation = nil
                if let error = error {
                    c.resume(throwing: ToolError.download(error.localizedDescription))
                    return
                }
                if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
                    c.resume(throwing: ToolError.download("HTTP \(http.statusCode)"))
                    return
                }
                guard let tmp = tmp else {
                    c.resume(throwing: ToolError.download("no file"))
                    return
                }
                let keep = dir.appendingPathComponent(UUID().uuidString)
                do {
                    try FileManager.default.moveItem(at: tmp, to: keep)
                    c.resume(returning: keep)
                } catch {
                    c.resume(throwing: ToolError.download(error.localizedDescription))
                }
            }
            state.observation = task.progress.observe(\.fractionCompleted, options: [.new]) { p, _ in
                progress(p.fractionCompleted)
            }
            task.resume()
        }
    }

    // MARK: Downloading videos

    /// Format order: resolution no higher than the screen; a codec the Mac decodes in hardware.
    static func formatSort() -> String {
        let target = VideoOptimizer.targetScreenPixels()
        let height = Int(min(2160, max(720, min(target.width, target.height))))
        if height <= 1080 { return "res:\(height),vcodec:h264" }
        if VTIsHardwareDecodeSupported(VideoConverter.av1) { return "res:\(height)" }
        return "res:\(height),vcodec:vp9"
    }

    final class Job {
        let process = Process()
        fileprivate var buffer = ""
        fileprivate var title: String?
        fileprivate var files: [String] = []
        fileprivate var formats: [String] = []
        fileprivate var reported = 0.0
        fileprivate var lastError: String?

        func cancel() {
            if process.isRunning { process.terminate() }
        }
    }

    /// A download that may take two attempts: first video + separate audio, then whatever single file there is.
    final class Fetch {
        fileprivate var job: Job?
        fileprivate var cancelled = false

        func cancel() {
            cancelled = true
            job?.cancel()
        }
    }

    /// Downloads a video page with yt-dlp. Sites like YouTube keep high-resolution video and the sound in separate
    /// streams; without ffmpeg yt-dlp can't join them, so both are downloaded and LiveWall joins them itself.
    /// The result lists the downloaded files (video, and audio when it came separately).
    static func fetchVideo(_ url: URL, into folder: URL, progress: @escaping (Double, String?) -> Void,
                           completion: @escaping (Result<([URL], String?), Error>) -> Void) -> Fetch {
        let fetch = Fetch()
        func attempt(_ formats: String, fallback: String?) {
            fetch.job = run(url, into: folder, formats: formats, progress: progress) { result in
                if case .failure(let error) = result, !fetch.cancelled, let next = fallback,
                   (error as? ToolError)?.isFormatUnavailable == true {
                    // no separate audio on this site: take the best single file
                    try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)
                        .forEach { try? FileManager.default.removeItem(at: $0) }
                    attempt(next, fallback: nil)
                    return
                }
                completion(result)
            }
        }
        attempt("bv*,ba[ext=m4a]", fallback: "bv*/b")
        return fetch
    }

    /// Runs yt-dlp once. `progress` and `completion` are called on the main queue.
    private static func run(_ url: URL, into folder: URL, formats: String,
                            progress: @escaping (Double, String?) -> Void,
                            completion: @escaping (Result<([URL], String?), Error>) -> Void) -> Job? {
        guard let exe = ytdlp else {
            DispatchQueue.main.async { completion(.failure(ToolError.failed(L("yt-dlp is not installed.")))) }
            return nil
        }
        let job = Job()
        let p = job.process
        p.executableURL = exe
        p.arguments = [
            "--no-playlist", "--newline", "--progress", "--no-part", "--no-mtime", "--no-colors",
            "-f", formats, "-S", formatSort(),
            "-o", folder.appendingPathComponent("%(id)s.f%(format_id)s.%(ext)s").path,
            "--progress-template",
            "download:LWPROG %(info.format_id)s %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s",
            "--print", "before_dl:LWTITLE %(title)s",
            "--print", "after_move:LWFILE %(filepath)s",
            "--", url.absoluteString,
        ]
        var env = ProcessInfo.processInfo.environment
        env["PATH"] = [dir.path, "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].joined(separator: ":")
        p.environment = env
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = pipe

        // called only on the main queue
        let consume: (String, Bool) -> Void = { text, final in
            job.buffer += text
            if final { job.buffer += "\n" }
            while let nl = job.buffer.firstIndex(where: { $0 == "\n" || $0 == "\r" }) {
                let line = String(job.buffer[..<nl]).trimmingCharacters(in: .whitespaces)
                job.buffer.removeSubrange(...nl)
                if line.hasPrefix("LWTITLE ") {
                    if job.title == nil {
                        job.title = String(line.dropFirst(8))
                        progress(0, job.title)
                    }
                } else if line.hasPrefix("LWFILE ") {
                    job.files.append(String(line.dropFirst(7)))
                } else if line.hasPrefix("ERROR:") {
                    job.lastError = String(line.dropFirst(6)).trimmingCharacters(in: .whitespaces)
                } else if line.hasPrefix("LWPROG ") {
                    let fields = line.dropFirst(7).split(separator: " ").map(String.init)
                    guard fields.count == 4, let done = Double(fields[1]),
                          let size = Double(fields[2]) ?? Double(fields[3]), size > 0 else { continue }
                    if !job.formats.contains(fields[0]) { job.formats.append(fields[0]) }
                    let part = job.formats.firstIndex(of: fields[0]) ?? 0
                    let v = min(1, done / size)
                    let scaled = part == 0 ? 0.9 * v : 0.9 + 0.1 * v
                    job.reported = min(1, max(job.reported, scaled))
                    progress(min(0.99, job.reported), nil)
                }
            }
        }
        let reader = pipe.fileHandleForReading
        reader.readabilityHandler = { handle in
            let chunk = handle.availableData
            guard !chunk.isEmpty else { return }
            let text = String(decoding: chunk, as: UTF8.self)
            DispatchQueue.main.async { consume(text, false) }
        }
        p.terminationHandler = { proc in
            reader.readabilityHandler = nil
            let rest = String(decoding: reader.readDataToEndOfFile(), as: UTF8.self)
            DispatchQueue.main.async {
                consume(rest, true)
                let files = job.files.filter { FileManager.default.fileExists(atPath: $0) }
                if proc.terminationReason == .uncaughtSignal {
                    completion(.failure(OptimizeError.cancelled))
                } else if proc.terminationStatus == 0, !files.isEmpty {
                    completion(.success((files.map { URL(fileURLWithPath: $0) }, job.title)))
                } else {
                    let text = job.lastError ?? LF("yt-dlp stopped with code %ld.", Int(proc.terminationStatus))
                    completion(.failure(ToolError.failed(text)))
                }
            }
        }
        do {
            try p.run()
        } catch {
            DispatchQueue.main.async { completion(.failure(ToolError.failed(error.localizedDescription))) }
            return nil
        }
        return job
    }
}
