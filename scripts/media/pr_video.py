#!/usr/bin/env python3
"""Knit の紹介動画(モーショングラフィックス版)を生成する。

実装済みの機能だけを描く(Android は「プレビュー」と明記)。無音・1920x1080・30fps。
使い方: python3 scripts/media/pr_video.py [出力.mp4]
既定の出力は docs/media/knit-pr.mp4。ffmpeg と Pillow が必要。
"""
import math
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

W, H, FPS = 1920, 1080, 30
DURATION = 42.0
ROOT = Path(__file__).resolve().parents[2]

# アイコンの配色(藍の地・水色・白)
BG_TOP = (30, 29, 72)
BG_BOT = (15, 15, 36)
ACCENT = (142, 162, 255)
WHITE = (238, 241, 255)
MUTED = (170, 176, 214)
SCREEN = (24, 26, 52)
BEZEL = (58, 62, 104)
WIN_BG = (36, 40, 78)
LINE = (70, 76, 128)


def font(size, weight="Medium"):
    for p in [
        Path.home() / f"Library/Fonts/NotoSansJP-{weight}.ttf",
        Path("/System/Library/Fonts/ヒラギノ角ゴシック W6.ttc"),
    ]:
        if p.exists():
            return ImageFont.truetype(str(p), size)
    return ImageFont.load_default()


def symfont(size):
    # ⌘ などの記号は SF で描く(Noto Sans JP に無い)
    p = Path("/System/Library/Fonts/SFNS.ttf")
    return ImageFont.truetype(str(p), size) if p.exists() else font(size)


F_TITLE = font(150, "Bold")
F_H1 = font(56, "Bold")
F_H2 = font(36, "Regular")
F_BODY = font(30, "Medium")
F_SMALL = font(24, "Regular")
F_KEY = symfont(40)
F_KEYJ = font(34, "Bold")

ICON = Image.open(ROOT / "assets/AppIcon.iconset/icon_512x512@2x.png").convert("RGBA")

# ---- 補間 ----


def clamp(v, a=0.0, b=1.0):
    return max(a, min(b, v))


def ease(t):
    t = clamp(t)
    return t * t * (3 - 2 * t)


def seg(t, a, b):
    """区間 [a,b] での進み具合 0..1"""
    return clamp((t - a) / (b - a)) if b > a else float(t >= b)


def fade(t, a, b, fin=0.4, fout=0.4):
    """区間 [a,b] で表示し、前後をフェードする不透明度"""
    if t < a or t > b:
        return 0.0
    return min(ease((t - a) / fin) if fin else 1, ease((b - t) / fout) if fout else 1)


def lerp(p, q, k):
    return (p[0] + (q[0] - p[0]) * k, p[1] + (q[1] - p[1]) * k)


def path_at(t, keys):
    """(時刻, 座標) のキーから位置を求める。同時刻の 2 キーは瞬間移動(境界を越える)"""
    if t <= keys[0][0]:
        return keys[0][1]
    for (t0, p0), (t1, p1) in zip(keys, keys[1:]):
        if t0 <= t <= t1:
            return p1 if t1 == t0 else lerp(p0, p1, ease((t - t0) / (t1 - t0)))
    return keys[-1][1]


# ---- 描画の部品 ----


def layer():
    return Image.new("RGBA", (W, H), (0, 0, 0, 0))


def put(base, lay, alpha):
    if alpha <= 0:
        return
    if alpha < 1:
        a = lay.getchannel("A").point(lambda v: int(v * alpha))
        lay.putalpha(a)
    base.alpha_composite(lay)


def text_c(d, xy, s, f, fill, anchor="mm"):
    d.text(xy, s, font=f, fill=fill, anchor=anchor)


def background():
    im = Image.new("RGBA", (W, H))
    d = ImageDraw.Draw(im)
    for y in range(H):
        k = y / H
        c = tuple(int(BG_TOP[i] + (BG_BOT[i] - BG_TOP[i]) * k) for i in range(3))
        d.line([(0, y), (W, y)], fill=c + (255,))
    return im


BG = background()

# 机の上の 3 台(画面の矩形)
MAC = (130, 250, 730, 625)
WIN = (790, 230, 1390, 605)
TAB = (1460, 330, 1820, 570)


def draw_devices(d, active):
    # Mac(ノート)
    x0, y0, x1, y1 = MAC
    d.rounded_rectangle((x0 - 14, y0 - 14, x1 + 14, y1 + 14), 18, fill=BEZEL)
    d.rounded_rectangle(MAC, 8, fill=SCREEN)
    d.polygon([(x0 - 60, y1 + 30), (x1 + 60, y1 + 30), (x1 + 20, y1 + 14), (x0 - 20, y1 + 14)], fill=BEZEL)
    d.rectangle((x0, y0, x1, y0 + 26), fill=(44, 48, 90))  # メニューバー
    d.rounded_rectangle((x0 + 50, y0 + 70, x1 - 60, y1 - 40), 10, fill=WIN_BG)
    for i, w in enumerate([380, 300, 420, 250]):
        d.rounded_rectangle((x0 + 80, y0 + 110 + i * 38, x0 + 80 + w, y0 + 124 + i * 38), 7, fill=LINE)
    text_c(d, ((x0 + x1) / 2, y1 + 70), "macOS", F_SMALL, MUTED)
    # Windows(モニター)
    x0, y0, x1, y1 = WIN
    d.rounded_rectangle((x0 - 14, y0 - 14, x1 + 14, y1 + 14), 14, fill=BEZEL)
    d.rounded_rectangle(WIN, 6, fill=SCREEN)
    d.rectangle(((x0 + x1) / 2 - 18, y1 + 14, (x0 + x1) / 2 + 18, y1 + 60), fill=BEZEL)
    d.rounded_rectangle(((x0 + x1) / 2 - 110, y1 + 56, (x0 + x1) / 2 + 110, y1 + 70), 6, fill=BEZEL)
    d.rectangle((x0, y1 - 30, x1, y1), fill=(44, 48, 90))  # タスクバー
    d.rounded_rectangle((x0 + 40, y0 + 40, x1 - 40, y1 - 60), 8, fill=WIN_BG)
    d.rectangle((x0 + 40, y0 + 40, x1 - 40, y0 + 70), fill=(52, 58, 108))
    text_c(d, ((x0 + x1) / 2, y1 + 100), "Windows", F_SMALL, MUTED)
    # Android タブレット(横置き)
    x0, y0, x1, y1 = TAB
    d.rounded_rectangle((x0 - 16, y0 - 16, x1 + 16, y1 + 16), 22, fill=BEZEL)
    d.rounded_rectangle(TAB, 10, fill=SCREEN)
    for i, (w, right) in enumerate([(170, False), (130, True), (190, False)]):
        bx = x1 - 30 - w if right else x0 + 30
        d.rounded_rectangle((bx, y0 + 30 + i * 46, bx + w, y0 + 62 + i * 46), 14, fill=(64, 70, 130) if right else LINE)
    d.rounded_rectangle((x0 + 24, y1 - 48, x1 - 24, y1 - 16), 14, outline=LINE, width=2)
    text_c(d, ((x0 + x1) / 2, y1 + 110), "Android", F_SMALL, MUTED)
    # いま操作している画面を光らせる
    if active:
        r = {"mac": MAC, "win": WIN, "tab": TAB}[active]
        for k, a in [(10, 60), (6, 120), (3, 255)]:
            d.rounded_rectangle((r[0] - k, r[1] - k, r[2] + k, r[3] + k), 12, outline=ACCENT + (a,), width=3)


def cursor(d, p, scale=1.0):
    x, y = p
    s = 1.6 * scale
    pts = [(0, 0), (0, 22), (6, 17), (10, 27), (14, 25), (10, 16), (17, 16)]
    pts = [(x + px * s, y + py * s) for px, py in pts]
    d.polygon(pts, fill=WHITE, outline=(20, 20, 40))


def keycap(d, center, label, f=None, alpha=255):
    f = f or F_KEY
    w = max(90, d.textlength(label, font=f) + 44)
    cx, cy = center
    d.rounded_rectangle((cx - w / 2, cy - 38, cx + w / 2, cy + 38), 14, fill=(236, 239, 255, alpha), outline=(120, 130, 200, alpha), width=3)
    d.text((cx, cy), label, font=f, fill=(30, 30, 70, alpha), anchor="mm")


def card(d, center, s, alpha=255):
    cx, cy = center
    w = d.textlength(s, font=F_BODY) + 50
    d.rounded_rectangle((cx - w / 2, cy - 32, cx + w / 2, cy + 32), 16, fill=(250, 251, 255, alpha))
    d.text((cx, cy), s, font=F_BODY, fill=(30, 30, 70, alpha), anchor="mm")


def file_icon(d, p, name, alpha=255):
    x, y = p
    d.polygon([(x, y), (x + 50, y), (x + 70, y + 20), (x + 70, y + 86), (x, y + 86)], fill=(236, 239, 255, alpha))
    d.polygon([(x + 50, y), (x + 50, y + 20), (x + 70, y + 20)], fill=(180, 188, 235, alpha))
    d.rounded_rectangle((x + 10, y + 52, x + 60, y + 72), 4, fill=ACCENT + (alpha,))
    d.text((x + 35, y + 106), name, font=F_SMALL, fill=WHITE + (alpha,), anchor="mm")


def ime_badge(d, p, s, on):
    x, y = p
    d.rounded_rectangle((x - 20, y - 14, x + 20, y + 14), 6, fill=ACCENT if on else (90, 96, 150))
    d.text((x, y), s, font=font(20, "Bold"), fill=(20, 20, 50), anchor="mm")


def caption(d, en, ja, alpha):
    if alpha <= 0:
        return
    a = int(255 * alpha)
    d.text((W / 2, 850), en, font=F_H1, fill=WHITE + (a,), anchor="mm")
    d.text((W / 2, 925), ja, font=F_H2, fill=MUTED + (a,), anchor="mm")


# ---- 台本(秒) ----
# 2: 画面の端で越える  3: コピー&貼り付け  4: 日本語入力  5: ファイル
# 6: Android(プレビュー)  7: 音  8: 安全と締め
CAPTIONS = [
    (4.0, 10.0, "Move to the edge. You're on the next computer.", "画面の端へ動かすだけで、隣の PC へ。"),
    (10.0, 16.0, "Copy on Mac. Paste on Windows.", "Mac でコピー、Windows で貼り付け。ショートカットはいつものまま。"),
    (16.0, 21.0, "Your Japanese input comes with you.", "日本語入力の状態も、一緒に移る。"),
    (21.0, 26.5, "Drag files across the border.", "ファイルを掴んだまま、境界を越える。"),
    (26.5, 32.0, "Android tablets, too.  (Preview)", "Android タブレットも、同じキーボードとマウスで(プレビュー)。"),
    (32.0, 36.5, "Hear your Windows PC on your Mac.", "Windows の音も、Mac のイヤホンで。"),
]

MAC_TXT = (420, 420)
WIN_TXT = (1090, 400)
TAB_IN = (1640, 540)
CUR = [
    (4.0, (430, 470)), (4.6, (430, 470)), (6.8, (727, 452)), (6.8, (793, 452)), (9.6, (1100, 440)),
    (10.2, (1100, 440)), (10.9, (793, 430)), (10.9, (727, 430)), (11.4, (470, 402)),
    (12.4, (470, 402)), (13.2, (727, 420)), (13.2, (793, 420)), (14.0, (1060, 396)),
    (16.0, (1060, 396)), (21.0, (1060, 396)),
    (21.8, (793, 470)), (21.8, (727, 470)), (22.4, (560, 520)),
    (22.9, (560, 520)), (24.0, (727, 470)), (24.0, (793, 470)), (25.0, (1040, 430)),
    (26.6, (1040, 430)), (27.8, (1387, 420)), (27.8, (1463, 420)), (28.6, (1600, 540)),
    (32.0, (1600, 540)),
]


def active_at(t):
    x = path_at(t, CUR)[0]
    if t < 4.6:
        return "mac"
    return "mac" if x < 760 else ("win" if x < 1425 else "tab")


def typed(s, t, a, b):
    return s[: int(len(s) * seg(t, a, b))]


def frame(t):
    im = BG.copy()
    d = ImageDraw.Draw(im, "RGBA")

    # 1: タイトル
    a = fade(t, 0.0, 4.2, fin=0.6, fout=0.6)
    if a:
        L = layer()
        ld = ImageDraw.Draw(L)
        ic = ICON.resize((220, 220), Image.LANCZOS)
        L.alpha_composite(ic, (W // 2 - 110, 190))
        text_c(ld, (W / 2, 560), "Knit", F_TITLE, WHITE)
        text_c(ld, (W / 2, 700), "One keyboard. One mouse. Every screen on your desk.", F_H1, WHITE)
        text_c(ld, (W / 2, 775), "机の上のすべての画面を、1 つのキーボードとマウスで。", F_H2, MUTED)
        put(im, L, a)

    # 2〜7: 机
    a = fade(t, 4.0, 36.8, fin=0.5, fout=0.5)
    if a:
        L = layer()
        ld = ImageDraw.Draw(L)
        draw_devices(ld, active_at(t))
        # 日本語入力の表示(Mac のメニューバー・Windows のタスクバー)
        kana_mac = t >= 16.6
        kana_win = t >= 17.4
        ime_badge(ld, (MAC[2] - 30, MAC[1] + 13), "あ" if kana_mac else "A", kana_mac)
        ime_badge(ld, (WIN[2] - 40, WIN[3] - 15), "あ" if kana_win else "A", kana_win)
        # Mac の文章と選択
        if 11.4 <= t:
            ld.rounded_rectangle((MAC[0] + 76, 392, MAC[0] + 466, 414), 5, fill=ACCENT + (90,))
        # Windows に貼られた文章と日本語入力
        if t >= 14.3:
            ld.text((WIN[0] + 70, 330), "Hello from my Mac", font=F_BODY, fill=WHITE, anchor="lm")
        if t >= 18.0:
            ld.text((WIN[0] + 70, 380), typed("こんにちは、Knit です", t, 18.0, 19.8), font=F_BODY, fill=WHITE, anchor="lm")
        # タブレットに打った文章
        if t >= 28.8:
            ld.text((TAB[0] + 44, TAB[3] - 32), typed("Meeting at 3?", t, 28.8, 30.6), font=F_SMALL, fill=WHITE, anchor="lm")
        put(im, L, a)

        L = layer()
        ld = ImageDraw.Draw(L)
        # 3: ⌘C → 貼り付け
        k = fade(t, 11.5, 12.6, 0.15, 0.3)
        if k:
            keycap(ld, (430, 200), "⌘C", alpha=int(255 * k))
        if 11.7 <= t <= 14.3:
            p = path_at(t, [(11.7, (430, 330)), (12.6, (430, 330)), (14.1, (1090, 330))])
            card(ld, p, "Hello from my Mac", alpha=int(255 * fade(t, 11.7, 14.3, 0.2, 0.2)))
        k = fade(t, 14.0, 15.4, 0.15, 0.3)
        if k:
            keycap(ld, (1090, 180), "⌘V  →  Ctrl+V", alpha=int(255 * k))
        # 4: かなキー
        k = fade(t, 16.4, 17.8, 0.15, 0.3)
        if k:
            keycap(ld, (430, 200), "かな", f=F_KEYJ, alpha=int(255 * k))
        # 5: ファイルを掴んで運ぶ
        if t < 22.9:
            if t >= 21.0:
                file_icon(ld, (525, 470), "report.pdf", int(255 * fade(t, 21.0, 22.95, 0.3, 0.05)))
        elif t < 25.1:
            p = path_at(t, CUR)
            file_icon(ld, (p[0] - 35, p[1] + 20), "report.pdf")
        k = fade(t, 25.1, 26.6, 0.2, 0.3)
        if k:
            x1, y1 = WIN[2], WIN[3]
            ld.rounded_rectangle((x1 - 360, y1 - 120, x1 - 20, y1 - 42), 12, fill=(250, 251, 255, int(245 * k)))
            ld.text((x1 - 340, y1 - 96), "report.pdf を受信しました", font=F_SMALL, fill=(30, 30, 70, int(255 * k)), anchor="lm")
            ld.text((x1 - 340, y1 - 64), "Downloads\\Knit", font=F_SMALL, fill=(90, 96, 150, int(255 * k)), anchor="lm")
        # 7: Windows の音が Mac のイヤホンへ
        k = fade(t, 32.0, 36.6, 0.4, 0.4)
        if k:
            hp = (430, 150)
            ld.arc((hp[0] - 50, hp[1] - 50, hp[0] + 50, hp[1] + 50), 180, 360, fill=WHITE + (int(255 * k),), width=10)
            for sx in (-50, 50):
                ld.rounded_rectangle((hp[0] + sx - 16, hp[1] - 6, hp[0] + sx + 16, hp[1] + 40), 8, fill=WHITE + (int(255 * k),))
            src = (1090, 180)
            for i in range(4):
                ph = ((t - 32.0) * 0.8 + i / 4) % 1.0
                p = lerp(src, hp, ph)
                r = 18 + 10 * math.sin(ph * math.pi)
                ld.arc((p[0] - r, p[1] - r, p[0] + r, p[1] + r), 200, 340, fill=ACCENT + (int(255 * k * math.sin(ph * math.pi)),), width=6)
            # Windows 側のスピーカーは消音
            sx, sy = WIN[2] - 90, WIN[3] - 15
            ld.polygon([(sx - 10, sy - 5), (sx - 4, sy - 5), (sx + 4, sy - 11), (sx + 4, sy + 11), (sx - 4, sy + 5), (sx - 10, sy + 5)], fill=WHITE + (int(255 * k),))
            ld.line((sx + 8, sy - 7, sx + 18, sy + 7), fill=(255, 120, 140, int(255 * k)), width=3)
            ld.line((sx + 18, sy - 7, sx + 8, sy + 7), fill=(255, 120, 140, int(255 * k)), width=3)
        # 境界を越えた瞬間: 入った側の画面の端を光らせる(同時刻に並ぶ 2 キー=越境)
        for (t0, p0), (t1, p1) in zip(CUR, CUR[1:]):
            k = 1 - seg(t, t0, t0 + 0.6) if t0 == t1 and t >= t0 else 0
            if k > 0:
                x = p1[0] - 4 if p1[0] > p0[0] else p1[0] + 4
                r = MAC if x < 760 else (WIN if x < 1425 else TAB)
                for w, al in [(18, 50), (10, 110), (4, 255)]:
                    ld.line((x, r[1] + 6, x, r[3] - 6), fill=ACCENT + (int(al * k),), width=w)
        if 4.6 <= t <= 36.4:
            cursor(ld, path_at(t, CUR))
        put(im, L, a)

    for c0, c1, en, ja in CAPTIONS:
        ca = fade(t, c0 + 0.2, c1 - 0.1, 0.35, 0.3)
        if ca:
            L = layer()
            caption(ImageDraw.Draw(L), en, ja, 1.0)
            put(im, L, ca)

    # 8: 安全 → 締め
    a = fade(t, 36.8, 39.9, fin=0.5, fout=0.4)
    if a:
        L = layer()
        ld = ImageDraw.Draw(L)
        text_c(ld, (W / 2, 300), "Private by design", F_H1, WHITE)
        text_c(ld, (W / 2, 370), "すべての通信を暗号化。つなぐのは、あなたの机の機器だけ。", F_H2, MUTED)
        items = [
            ("Encrypted", "Noise プロトコルで全経路を暗号化"),
            ("Local only", "LAN・直結・Tailscale のみ"),
            ("6-digit pairing", "6 桁のコードで登録"),
        ]
        for i, (en, ja) in enumerate(items):
            cx = W / 2 + (i - 1) * 520
            ld.rounded_rectangle((cx - 230, 470, cx + 230, 650), 24, fill=(44, 48, 96, 255), outline=ACCENT, width=2)
            text_c(ld, (cx, 535), en, F_BODY, WHITE)
            text_c(ld, (cx, 595), ja, F_SMALL, MUTED)
        put(im, L, a)
    a = fade(t, 39.8, DURATION + 1, fin=0.6, fout=0)
    if a:
        L = layer()
        ld = ImageDraw.Draw(L)
        ic = ICON.resize((170, 170), Image.LANCZOS)
        L.alpha_composite(ic, (W // 2 - 85, 210))
        text_c(ld, (W / 2, 470), "Knit", font(120, "Bold"), WHITE)
        text_c(ld, (W / 2, 590), "One keyboard. One mouse. Every screen on your desk.", F_BODY, WHITE)
        text_c(ld, (W / 2, 660), "Open source (MIT)  ·  macOS  ·  Windows  ·  Android (preview)", F_SMALL, MUTED)
        text_c(ld, (W / 2, 760), "github.com/reoanrudo/knit", F_H2, ACCENT)
        put(im, L, a)
    return im.convert("RGB")


def main():
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "docs/media/knit-pr.mp4"
    out.parent.mkdir(parents=True, exist_ok=True)
    if len(sys.argv) > 2 and sys.argv[2] == "--stills":
        # 確認用: 各場面の静止画を書き出す
        for s in [2.0, 6.9, 7.5, 13.5, 18.8, 24.0, 30.0, 34.5, 38.5, 41.0]:
            frame(s).save(out.parent / f"still-{s:04.1f}.png")
        return
    n = int(DURATION * FPS)
    ff = subprocess.Popen(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24",
         "-s", f"{W}x{H}", "-r", str(FPS), "-i", "-", "-c:v", "libx264", "-preset", "slow",
         "-crf", "20", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(out)],
        stdin=subprocess.PIPE,
    )
    for i in range(n):
        ff.stdin.write(frame(i / FPS).tobytes())
        if i % (FPS * 5) == 0:
            print(f"{i / FPS:5.1f}s / {DURATION:.0f}s", flush=True)
    ff.stdin.close()
    sys.exit(ff.wait())


if __name__ == "__main__":
    main()
