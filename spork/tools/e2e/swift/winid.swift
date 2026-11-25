import CoreGraphics
import Foundation

// usage: winid <pid> [smallest] — the id of that process's largest (or smallest) on-screen
// window, for screencapture -l
let pid = Int(CommandLine.arguments[1]) ?? -1
let smallest = CommandLine.arguments.count > 2 && CommandLine.arguments[2] == "smallest"
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
    as? [[String: Any]] ?? []
var best: (id: Int, area: Double)?
for window in windows where window[kCGWindowOwnerPID as String] as? Int == pid
    && window[kCGWindowLayer as String] as? Int == 0 {
    let bounds = window[kCGWindowBounds as String] as? [String: Double] ?? [:]
    let area = (bounds["Width"] ?? 0) * (bounds["Height"] ?? 0)
    guard area > 2000 else { continue }
    if best == nil || (smallest ? area < best!.area : area > best!.area) { best = (window[kCGWindowNumber as String] as? Int ?? 0, area) }
}
guard let best else { exit(1) }
print(best.id)
