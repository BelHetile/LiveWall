import AppKit

@MainActor
public protocol PlaybackNode: AnyObject {
    var view: NSView { get }
    var isPlaying: Bool { get }
    func play()
    func pause()
}
