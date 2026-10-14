import Foundation

public enum LiveWallError: Error, Equatable, Sendable {
    case mediaUnreadable(path: String, reason: String)
    case codecUnsupported(path: String)
    case downloadFailed(jobID: String, stderr: String)
    case toolMissing(tool: String)
    case toolAcquisitionFailed(tools: [String], stderr: String)
    case consentRequired
    case libraryCorrupt(path: String)
    case ipc(code: String, message: String)
    case swapStateCorrupt(path: String)
    case stillUnavailable(path: String, reason: String)
}
