//! カーソル近傍に出す小さな案内の位置決め(I3)。
//!
//! なぜ: 掴みを断った理由は通知バナー(osascript)でしか出しておらず、**カーソルの
//! 近くに出ず、数秒で消える**(実測: 起動に 133〜255ms かかる)。ユーザーは端に何度も
//! 当て直し、最後は「壊れた」と誤認して ⌘C 運用へ逃げていた(摩擦 F1)。
//!
//! ここは**計算だけ**を置く。表示の可否・文言は呼び出し側が与える値で決まり、
//! 座標系は AppKit と同じ左下原点(y が上向き)にそろえる。
use crate::drag::{Refusal, MAX_FILES};

/// 断った理由の1行(通知本文より短く、その場で読める長さ)
impl Refusal {
    pub fn hud_line(&self) -> String {
        match self {
            Refusal::TooMany => format!("掴みすぎです({} 件まで)", MAX_FILES),
            Refusal::EmptyGrab => "掴んだ項目がありません".to_string(),
            Refusal::LinkDown => "転送の準備中です(少し待って掴み直す)".to_string(),
            Refusal::CollectStale => "接続が変わったため掴み直してください".to_string(),
            Refusal::CollectTimeout => "確認が間に合いません(少し待って掴み直す)".to_string(),
            Refusal::CollectFailed { label } => format!("渡せません({label})"),
            Refusal::NothingToSend => "送れる項目がありません".to_string(),
            Refusal::Busy => "転送中です(完了後にもう一度)".to_string(),
        }
    }
}

/// 断ったときに必ず添える一言。**そのまま離しても Mac 内の操作は壊れない**ことを伝える
/// (断りは「越えさせない」だけで、掴んでいるドラッグ自体は Mac 側で生きている)
pub const HUD_HINT: &str = "このまま離すと Mac 内の操作になります";

/// パネルの位置。`cursor` は左下原点の画面座標、`size` はパネルの大きさ、
/// `screen` は (x, y, w, h)。**右下(視覚的には右上)を優先**し、入らなければ
/// 左・下へ反転、それでも入らなければ画面内へ寄せる
pub fn place(cursor: (f64, f64), size: (f64, f64), screen: (f64, f64, f64, f64)) -> (f64, f64) {
    const GAP: f64 = 18.0;
    let (cx, cy) = cursor;
    let (w, h) = size;
    let (sx, sy, sw, sh) = screen;
    // 画面より大きいパネルは左上に合わせる(はみ出しは許容し、原点だけ決める)
    let max_x = (sx + sw - w).max(sx);
    let max_y = (sy + sh - h).max(sy);
    let mut x = cx + GAP;
    let mut y = cy + GAP;
    if x + w > sx + sw {
        x = cx - GAP - w; // 右に入らなければ左へ
    }
    if y + h > sy + sh {
        y = cy - GAP - h; // 上に入らなければ下へ
    }
    (x.clamp(sx, max_x), y.clamp(sy, max_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (f64, f64, f64, f64) = (0.0, 0.0, 1440.0, 900.0);
    const SIZE: (f64, f64) = (320.0, 44.0);

    fn inside(p: (f64, f64)) -> bool {
        let (x, y) = p;
        x >= SCREEN.0 && y >= SCREEN.1 && x + SIZE.0 <= SCREEN.0 + SCREEN.2 && y + SIZE.1 <= SCREEN.1 + SCREEN.3
    }

    #[test]
    fn prefers_the_upper_right_of_the_cursor() {
        let p = place((600.0, 400.0), SIZE, SCREEN);
        assert!(p.0 > 600.0 && p.1 > 400.0, "右上に出す: {p:?}");
        assert!(inside(p), "{p:?}");
    }

    #[test]
    fn flips_at_every_corner_to_stay_on_screen() {
        for cursor in [
            (10.0, 10.0),       // 左下
            (1430.0, 10.0),     // 右下
            (10.0, 890.0),      // 左上
            (1430.0, 890.0),    // 右上
            (720.0, 450.0),     // 中央
        ] {
            let p = place(cursor, SIZE, SCREEN);
            assert!(inside(p), "画面内に収まる cursor={cursor:?} -> {p:?}");
        }
    }

    #[test]
    fn works_on_a_second_display_with_negative_origin() {
        // メインディスプレイの左に置いた外部モニター(x が負)
        let screen = (-1920.0, 0.0, 1920.0, 1080.0);
        let p = place((-100.0, 900.0), SIZE, screen);
        assert!(p.0 >= -1920.0 && p.0 + SIZE.0 <= 0.0, "{p:?}");
        assert!(p.1 >= 0.0 && p.1 + SIZE.1 <= 1080.0, "{p:?}");
    }

    #[test]
    fn a_panel_larger_than_the_screen_keeps_the_origin_inside() {
        let big = (2000.0, 1200.0);
        let p = place((700.0, 450.0), big, SCREEN);
        assert_eq!(p, (0.0, 0.0), "収まらないときは画面の原点へ寄せる");
    }

    #[test]
    fn the_line_is_short_and_never_claims_success() {
        for r in [
            Refusal::TooMany,
            Refusal::EmptyGrab,
            Refusal::LinkDown,
            Refusal::CollectStale,
            Refusal::CollectTimeout,
            Refusal::CollectFailed {
                label: "展開後 640 件以上(上限 512 件)".to_string(),
            },
            Refusal::NothingToSend,
            Refusal::Busy,
        ] {
            let line = r.hud_line();
            assert!(!line.is_empty(), "{r:?}");
            assert!(line.chars().count() <= 40, "1行で読める: {line}");
            for w in crate::notice::PLACEMENT_CLAIMS {
                assert!(!line.contains(w), "置けたと言わない: {line}");
            }
        }
        // 空の掴みでは「64 件まで」と言わない(理由の粒度を保つ)
        assert_ne!(
            Refusal::EmptyGrab.hud_line(),
            Refusal::TooMany.hud_line(),
            "0 件と 64 件超は別の案内"
        );
        assert!(HUD_HINT.contains("Mac 内"), "{HUD_HINT}");
    }
}
