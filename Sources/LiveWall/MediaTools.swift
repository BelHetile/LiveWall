import AppKit
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
    static var installed: Bool { ytdlp != nil && deno != nil }
    /// Downloaded by LiveWall itself (so they can be updated), not from Homebrew.
    static var ownCopy: Bool { FileManager.default.fileExists(atPath: dir.appendingPathComponent("yt-dlp").path) }

    /// Sites that don't serve videos as files: go straight to yt-dlp.
    static func isVideoSite(_ url: URL) -> Bool {
        guard let host = url.host?.lowercased() else { return false }
        let sites = ["youtube.com", "youtu.be", "youtube-nocookie.com", "vimeo.com", "tiktok.com", "twitch.tv",
                     "dailymotion.com", "bilibili.com", "b23.tv", "x.com", "twitter.com", "instagram.com",
                     "facebook.com", "fb.watch", "reddit.com", "redd.it", "streamable.com", "vk.com",
                     "vkvideo.ru", "rutube.ru", "pinterest.com", "pin.it"]
        return sites.contains(where: { host == $0 || host.hasSuffix("." + $0) })
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
    static func install(progress: @escaping (Double) -> Void) async throws {
        let ytURL = URL(string: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos")!
        #if arch(arm64)
        let denoName = "deno-aarch64-apple-darwin.zip"
        #else
        let denoName = "deno-x86_64-apple-darwin.zip"
        #endif
        let denoURL = URL(string: "https://github.com/denoland/deno/releases/latest/download/\(denoName)")!

        let yt = try await download(ytURL) { progress($0 * 0.4) }
        let ytDest = dir.appendingPathComponent("yt-dlp")
        try? FileManager.default.removeItem(at: ytDest)
        try FileManager.default.moveItem(at: yt, to: ytDest)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: ytDest.path)

        let zip = try await download(denoURL) { progress(0.4 + $0 * 0.55) }
        defer { try? FileManager.default.removeItem(at: zip) }
        let unpack = dir.appendingPathComponent("deno-unpack", isDirectory: true)
        try? FileManager.default.removeItem(at: unpack)
        let status = try runQuiet(URL(fileURLWithPath: "/usr/bin/ditto"), ["-x", "-k", zip.path, unpack.path])
        guard status == 0 else { throw ToolError.download("unzip") }
        let denoDest = dir.appendingPathComponent("deno")
        try? FileManager.default.removeItem(at: denoDest)
        try FileManager.default.moveItem(at: unpack.appendingPathComponent("deno"), to: denoDest)
        try? FileManager.default.removeItem(at: unpack)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: denoDest.path)
        progress(1)
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
        fileprivate var part = 0          // which of the downloads is running (0-based)
        fileprivate var lastPercent = 0.0
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
        func attempt(_ formats: String, parts: Int, fallback: String?) {
            fetch.job = run(url, into: folder, formats: formats, parts: parts, progress: progress) { result in
                if case .failure(let error) = result, !fetch.cancelled, let next = fallback,
                   !(error is OptimizeError) {
                    // no separate audio on this site: take the best single file
                    try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)
                        .forEach { try? FileManager.default.removeItem(at: $0) }
                    attempt(next, parts: 1, fallback: nil)
                    return
                }
                completion(result)
            }
        }
        attempt("bv*,ba[ext=m4a]", parts: 2, fallback: "bv*/b")
        return fetch
    }

    /// Runs yt-dlp once. `progress` and `completion` are called on the main queue.
    private static func run(_ url: URL, into folder: URL, formats: String, parts: Int,
                            progress: @escaping (Double, String?) -> Void,
                            completion: @escaping (Result<([URL], String?), Error>) -> Void) -> Job? {
        guard let exe = ytdlp else {
            completion(.failure(ToolError.failed(L("yt-dlp is not installed."))))
            return nil
        }
        let job = Job()
        let p = job.process
        p.executableURL = exe
        p.arguments = [
            "--no-playlist", "--newline", "--progress", "--no-part", "--no-mtime", "--no-colors",
            "-f", formats, "-S", formatSort(),
            "-o", folder.appendingPathComponent("%(id)s.f%(format_id)s.%(ext)s").path,
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

        let regex = try? NSRegularExpression(pattern: "\\[download\\]\\s+([0-9.]+)%")
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
                } else if let re = regex,
                          let m = re.firstMatch(in: line, range: NSRange(line.startIndex..., in: line)),
                          let r = Range(m.range(at: 1), in: line), let v = Double(line[r]) {
                    // the percentage starting over means the next file (the sound) is downloading
                    if v + 30 < job.lastPercent { job.part += 1 }
                    job.lastPercent = v
                    let done = Double(min(job.part, parts - 1))
                    progress(min(1, (done + v / 100) / Double(parts)), nil)
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
            completion(.failure(ToolError.failed(error.localizedDescription)))
            return nil
        }
        return job
    }
}
