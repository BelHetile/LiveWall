import AppKit
import Foundation
import LiveWallCore
import LiveWallEngine

let arguments = CommandLine.arguments
guard arguments.count > 3 else {
    FileHandle.standardError.write(
        Data("usage: LiveWallStillProbe <image> <swap-state-path> <stills-dir>\n".utf8)
    )
    exit(2)
}

let itemURL = URL(fileURLWithPath: arguments[1])
let swapURL = URL(fileURLWithPath: arguments[2])
let stillsURL = URL(fileURLWithPath: arguments[3])

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

var finished = false
var failure: Error?

let takeover = Task { @MainActor in
    do {
        let service = StillSyncService(
            workspace: NSWorkspaceDesktopPicture(),
            store: SwapStateStore(url: swapURL),
            stillDirectory: stillsURL
        )
        try await service.takeover(StillRequest(url: itemURL, kind: .image))
        print("TOOK OVER \(service.currentStillURL?.path ?? "nil")")
        fflush(stdout)
    } catch {
        failure = error
    }
    finished = true
}
_ = takeover

while !finished {
    RunLoop.main.run(until: Date().addingTimeInterval(0.05))
}

if let failure {
    FileHandle.standardError.write(Data("error: \(failure)\n".utf8))
    exit(3)
}

RunLoop.main.run()
