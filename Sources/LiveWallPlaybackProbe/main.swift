import AppKit
import Foundation
import LiveWallEngine

let arguments = CommandLine.arguments
guard arguments.count > 1 else {
    FileHandle.standardError.write(Data("usage: LiveWallPlaybackProbe <video> [timeout-seconds]\n".utf8))
    exit(2)
}

let url = URL(fileURLWithPath: arguments[1])
let timeout = arguments.count > 2 ? (Double(arguments[2]) ?? 3.0) : 3.0

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

var node: VideoNode?
var failure: Error?
let creation = Task { @MainActor in
    do {
        node = try await VideoNode(url: url)
    } catch {
        failure = error
    }
}
_ = creation
while node == nil && failure == nil {
    RunLoop.main.run(until: Date().addingTimeInterval(0.05))
}

if let failure {
    FileHandle.standardError.write(Data("error: \(failure)\n".utf8))
    exit(3)
}

guard let node else {
    FileHandle.standardError.write(Data("error: node not created\n".utf8))
    exit(3)
}

guard let screen = NSScreen.main ?? NSScreen.screens.first else {
    FileHandle.standardError.write(Data("error: no screen\n".utf8))
    exit(4)
}

let window = WallpaperWindow(displayID: 1, frame: screen.frame)
window.installContentView(node.view)
window.orderFrontRegardless()
node.play()

let deadline = Date().addingTimeInterval(timeout)
while node.currentTime.seconds <= 0.2 && Date() < deadline {
    RunLoop.main.run(until: Date().addingTimeInterval(0.05))
}

let seconds = node.currentTime.seconds
let ready = node.playerLayer.isReadyForDisplay
print("PLAYBACK time=\(seconds) ready=\(ready) status=\(node.player.timeControlStatus.rawValue)")

node.pause()
window.orderOut(nil)
window.close()

exit(seconds > 0.1 ? 0 : 1)
