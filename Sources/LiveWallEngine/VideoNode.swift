import AVFoundation
import AppKit
import Foundation
import LiveWallCore

@MainActor
public final class VideoNode: PlaybackNode {
    public let player: AVQueuePlayer
    public let playerLayer: AVPlayerLayer
    public let view: NSView
    public let asset: AVURLAsset

    private let looper: AVPlayerLooper
    private var playbackRate: Float

    public init(url: URL, muted: Bool = true, volume: Float = 1.0, rate: Float = 1.0) async throws {
        let asset = try await VideoNode.validate(url: url)
        let player = AVQueuePlayer()
        player.isMuted = muted
        player.volume = min(max(volume, 0), 1)
        player.automaticallyWaitsToMinimizeStalling = false
        player.rate = 0
        let item = AVPlayerItem(asset: asset)
        let looper = AVPlayerLooper(player: player, templateItem: item)
        let layer = AVPlayerLayer(player: player)
        layer.videoGravity = .resizeAspectFill
        let view = NSView(frame: .zero)
        view.wantsLayer = true
        view.layer = layer

        self.asset = asset
        self.player = player
        self.playerLayer = layer
        self.view = view
        self.looper = looper
        self.playbackRate = rate > 0 ? rate : 1.0
    }

    public var isPlaying: Bool {
        return player.rate != 0
    }

    public var isMuted: Bool {
        get { return player.isMuted }
        set { player.isMuted = newValue }
    }

    public var volume: Float {
        get { return player.volume }
        set { player.volume = min(max(newValue, 0), 1) }
    }

    public var rate: Float {
        return playbackRate
    }

    public var videoGravity: AVLayerVideoGravity {
        get { return playerLayer.videoGravity }
        set { playerLayer.videoGravity = newValue }
    }

    public var currentTime: CMTime {
        return player.currentTime()
    }

    public func play() {
        player.play()
        if playbackRate != 1.0 {
            player.rate = playbackRate
        }
    }

    public func pause() {
        player.pause()
    }

    public func setRate(_ rate: Float) {
        playbackRate = rate > 0 ? rate : 1.0
        if player.rate != 0 {
            player.rate = playbackRate
        }
    }

    public func setMuted(_ muted: Bool) {
        player.isMuted = muted
    }

    public func setVolume(_ volume: Float) {
        player.volume = min(max(volume, 0), 1)
    }

    static func validate(url: URL) async throws -> AVURLAsset {
        let path = url.path
        guard FileManager.default.fileExists(atPath: path) else {
            throw LiveWallError.mediaUnreadable(path: path, reason: "missing file")
        }
        let attributes = try? FileManager.default.attributesOfItem(atPath: path)
        let fileSize = (attributes?[.size] as? NSNumber)?.int64Value ?? 0
        guard fileSize > 0 else {
            throw LiveWallError.mediaUnreadable(path: path, reason: "empty file")
        }

        let asset = AVURLAsset(url: url)
        let duration: CMTime
        do {
            duration = try await asset.load(.duration)
        } catch {
            throw LiveWallError.mediaUnreadable(path: path, reason: "unreadable asset")
        }
        guard duration.isValid, duration.seconds > 0 else {
            throw LiveWallError.mediaUnreadable(path: path, reason: "invalid duration")
        }

        let tracks: [AVAssetTrack]
        do {
            tracks = try await asset.loadTracks(withMediaType: .video)
        } catch {
            throw LiveWallError.mediaUnreadable(path: path, reason: "unreadable tracks")
        }
        guard let track = tracks.first else {
            throw LiveWallError.mediaUnreadable(path: path, reason: "no video track")
        }

        let decodable = (try? await track.load(.isDecodable)) ?? false
        guard decodable else {
            throw LiveWallError.codecUnsupported(path: path)
        }

        let sampleDataLength = (try? await track.load(.totalSampleDataLength)) ?? 0
        guard sampleDataLength <= fileSize else {
            throw LiveWallError.mediaUnreadable(path: path, reason: "truncated sample data")
        }

        return asset
    }
}
