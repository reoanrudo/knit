// poc/mac_input_poc.swift
// CGEventTapによるキー/マウスイベントのフック検証
// 5秒間イベントを観測して終了。抑制(return nil)はオプションで試す。

import ApplicationServices
import Carbon.HIToolbox
import CoreGraphics
import Foundation

let suppressKeys = CommandLine.arguments.contains("--suppress")

print("AXIsProcessTrusted: \(AXIsProcessTrusted())")

let mask = (1 << CGEventType.keyDown.rawValue)
        | (1 << CGEventType.keyUp.rawValue)
        | (1 << CGEventType.mouseMoved.rawValue)
        | (1 << CGEventType.leftMouseDown.rawValue)

var keyCount = 0
var mouseCount = 0
var suppressedCount = 0

let tap = CGEvent.tapCreate(
    tap: .cgSessionEventTap,
    place: .headInsertEventTap,
    options: suppressKeys ? .defaultTap : .listenOnly,
    eventsOfInterest: CGEventMask(mask),
    callback: { proxy, type, event, refcon -> Unmanaged<CGEvent>? in
        switch type {
        case .keyDown, .keyUp:
            keyCount += 1
            let keyCode = event.getIntegerValueField(.keyboardEventKeycode)
            let flags = event.flags
            print("[key] type=\(type == .keyDown ? "down" : "up") code=\(keyCode) cmd=\(flags.contains(.maskCommand))")
            if suppressKeys && keyCode == kVK_F13 { // F13で抑制テスト
                suppressedCount += 1
                print("  -> suppressed F13")
                return nil
            }
        case .mouseMoved:
            mouseCount += 1
        case .leftMouseDown:
            print("[mouse] left down at (\(event.location.x), \(event.location.y))")
        default:
            break
        }
        return Unmanaged.passRetained(event)
    },
    userInfo: nil
)

guard let tap else {
    print("FAILED: CGEvent.tapCreate returned nil (アクセシビリティ権限なし)")
    exit(1)
}
print("tap created OK. observing 5s...")

CFRunLoopAddSource(CFRunLoopGetCurrent(), CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0), .commonModes)
CGEvent.tapEnable(tap: tap, enable: true)

Thread.detachNewThread {
    Thread.sleep(forTimeInterval: 1)
    // 合成マウス移動イベントを10回postしてフック経路を自己検証
    let loc = CGEvent(source: nil)?.location ?? CGPoint(x: 500, y: 500)
    for i in 0..<10 {
        let ev = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved,
                         mouseCursorPosition: CGPoint(x: loc.x + CGFloat(i), y: loc.y),
                         mouseButton: .left)
        ev?.post(tap: .cghidEventTap)
        usleep(50_000)
    }
    Thread.sleep(forTimeInterval: 1)
    print("done. keys=\(keyCount) mouseMoves=\(mouseCount) suppressed=\(suppressedCount)")
    exit(0)
}
RunLoop.main.run()
