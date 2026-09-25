#!/bin/bash
# カーソル凍結(抑制)の自動検証: 切替後に Mac カーソルが動かなければ PASS
# 使い方: ./scripts/check-mouse.sh   (sd-mac 起動中に実行)
cd "$(dirname "$0")/.."

swift /dev/stdin <<'EOF'
import CoreGraphics
import AppKit

func pos() -> String { "\(NSEvent.mouseLocation)" }

// 1) 右端到達イベントを投稿して切替を発火
if let ev = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: CGPoint(x: 2055, y: 400), mouseButton: .left) {
    ev.post(tap: .cghidEventTap)
}
Thread.sleep(forTimeInterval: 0.8)
let p1 = NSEvent.mouseLocation
print("after-switch: \(p1)")

// 2) 大きな移動イベントを数回投稿(凍結されていれば動かない)
var moved = 0.0
for _ in 0..<4 {
    if let ev = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: CGPoint(x: 300.0, y: 900.0), mouseButton: .left) {
        ev.post(tap: .cghidEventTap)
    }
    Thread.sleep(forTimeInterval: 0.25)
    let p = NSEvent.mouseLocation
    moved += abs(p.x - p1.x) + abs(p.y - p1.y)
    print("probe: \(p)")
}

// 3) 判定: ほぼ動いていなければ凍結成功
if moved < 20 {
    print("RESULT: FROZEN (抑制OK)")
} else {
    print("RESULT: MOVING (抑制NG, moved=\(moved))")
}
EOF

echo "--- sd-mac log ---"
tail -3 /tmp/sd-mac-run.log
