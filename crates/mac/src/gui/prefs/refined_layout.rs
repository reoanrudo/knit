//! 新設定画面の配置キャンバス。選択と編集中の値は入力先/OSの画面配置と独立。
//! AppKitの呼出しはメインスレッド限定。Mutex保持中にUI callbackを呼ばない。
use super::*;

const MIN_RANGE: f64 = 0.01;
const HIT_RADIUS: f64 = 9.0; // 端点18pt、帯16pt。Retinaでも12pxを下回らない。
static CANVAS: AtomicUsize = AtomicUsize::new(0);
static IDENTIFY_UNTIL: AtomicU64 = AtomicU64::new(0);
static LAST_RUNTIME_SYNC: AtomicU64 = AtomicU64::new(0);
static DRAFT: Mutex<Option<LayoutDraft>> = Mutex::new(None);

type Point = (f64, f64);

#[derive(Clone, PartialEq)]
struct MacStamp {
    geometry: [u64; 4],
    name: String,
    built_in: bool,
}

#[derive(Clone, PartialEq)]
struct PeerStamp {
    id: String,
    generation: u64,
    screen: [u64; 2],
    monitors: Vec<(i32, i32, i32, i32, String)>,
    // 接続/構成以外の経路から配置が変わった時も古いdraftで上書きしない。
    side: u8,
    monitor: Option<usize>,
    layout: Vec<[u64; 4]>,
}

#[derive(Clone, PartialEq)]
struct Topology {
    preview: bool,
    macs: Vec<MacStamp>,
    peers: Vec<PeerStamp>,
}

#[derive(Clone)]
struct PeerDraft {
    id: String,
    name: String,
    tablet: bool,
    rects: Vec<Rect>,
    size: Point,
    side: u8,
    monitor: Option<usize>,
    settings: crate::screen_settings::PeerSettings,
    original_side: u8,
    original_monitor: Option<usize>,
    original_settings: crate::screen_settings::PeerSettings,
}

impl PeerDraft {
    fn dirty(&self) -> bool {
        self.side != self.original_side
            || self.monitor != self.original_monitor
            || self.settings != self.original_settings
    }

    fn accept(&mut self) {
        self.original_side = self.side;
        self.original_monitor = self.monitor;
        self.original_settings = self.settings;
    }
}

#[derive(Clone)]
struct LayoutDraft {
    topology: Topology,
    macs: Vec<Rect>,
    mac_size: Point,
    mac_name: String,
    built_in: Vec<bool>,
    peers: Vec<PeerDraft>,
    selected: Option<String>,
    drag: Option<Drag>,
}

#[derive(Clone, Copy)]
enum RangePart {
    Start,
    End,
    Body,
}

#[derive(Clone, Copy)]
enum DragKind {
    Device {
        grab: Point,
    },
    Range {
        source: bool,
        part: RangePart,
        edge: Edge,
        start: f64,
    },
}

#[derive(Clone)]
struct Drag {
    id: String,
    before: PeerDraft,
    kind: DragKind,
    down: Point,
    center: Point,
    moved: bool,
    // 描画fitをドラッグ開始時に固定し、ドラッグ中の再fitによる跳躍を防ぐ。
    scene: Scene,
}

#[derive(Clone)]
struct Device {
    id: Option<String>,
    rects: Vec<NSRect>,
    bounds: NSRect,
}

#[derive(Clone)]
struct Scene {
    mac: Device,
    peers: Vec<Device>,
}

#[derive(Clone, Copy)]
struct Edge {
    rect: NSRect,
    dir: u8,
}

impl Edge {
    // 正規化は常に上→下/左→右。相手側でも反転しない。
    fn point(self, value: f64) -> Point {
        match self.dir {
            0 => (
                self.rect.x + self.rect.w,
                self.rect.y + self.rect.h * (1.0 - value),
            ),
            1 => (self.rect.x, self.rect.y + self.rect.h * (1.0 - value)),
            2 => (self.rect.x + self.rect.w * value, self.rect.y + self.rect.h),
            _ => (self.rect.x + self.rect.w * value, self.rect.y),
        }
    }

    fn position(self, point: Point) -> f64 {
        if self.dir < 2 {
            1.0 - (point.1 - self.rect.y) / self.rect.h.max(f64::EPSILON)
        } else {
            (point.0 - self.rect.x) / self.rect.w.max(f64::EPSILON)
        }
    }
}

fn lock_draft() -> std::sync::MutexGuard<'static, Option<LayoutDraft>> {
    DRAFT.lock().unwrap_or_else(|e| e.into_inner())
}

fn rect_bits(r: Rect) -> [u64; 4] {
    [r.0.to_bits(), r.1.to_bits(), r.2.to_bits(), r.3.to_bits()]
}

fn valid_rect(r: Rect) -> bool {
    [r.0, r.1, r.2, r.3].iter().all(|v| v.is_finite()) && r.2 > 0.0 && r.3 > 0.0
}

fn clean_group(group: RectGroup) -> RectGroup {
    // 実runtimeで未知の画面を補完しない。0画面は0画面として扱う。
    let list: Vec<_> = group.0.into_iter().filter(|r| valid_rect(*r)).collect();
    if list.is_empty() {
        (Vec::new(), (0.0, 0.0))
    } else {
        let w = list.iter().map(|r| r.0 + r.2).fold(0.0, f64::max);
        let h = list.iter().map(|r| r.1 + r.3).fold(0.0, f64::max);
        (list, (w, h))
    }
}

/// CoreGraphicsの列挙番号はMacDisplayにないため、boundsで内蔵判定を突き合わせる。
fn builtin_flags(displays: &[crate::MacDisplay]) -> Vec<bool> {
    unsafe {
        let mut ids = [0u32; 16];
        let mut count = 0;
        if crate::CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut count) != 0 {
            return vec![false; displays.len()];
        }
        displays
            .iter()
            .map(|d| {
                ids[..(count as usize).min(ids.len())].iter().any(|id| {
                    let r = crate::CGDisplayBounds(*id);
                    CGDisplayIsBuiltin(*id) != 0
                        && (r.origin.x - d.x).abs() < 0.5
                        && (r.origin.y - d.y).abs() < 0.5
                        && (r.size.w - d.w).abs() < 0.5
                        && (r.size.h - d.h).abs() < 0.5
                })
            })
            .collect()
    }
}

fn runtime_draft() -> LayoutDraft {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return preview_draft();
    }
    let displays = crate::mac_displays();
    let built_in = builtin_flags(&displays);
    let macs: Vec<_> = displays
        .iter()
        .enumerate()
        .map(|(i, d)| MacStamp {
            geometry: rect_bits((d.x, d.y, d.w, d.h)),
            name: d.name.clone(),
            built_in: built_in[i],
        })
        .collect();
    // mac_groupと同じ正規化。ここで取得した同一snapshotから描画と世代検証を作る。
    let raw: Vec<_> = displays.iter().map(|d| (d.x, d.y, d.w, d.h)).collect();
    let (rects, size) = clean_group(normalize_monitors(&raw));
    let (stamps, mut peers): (Vec<_>, Vec<_>) = {
        let live = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        live.iter()
            .map(|p| {
                let group = if p.monitors.is_empty()
                    && (!p.screen.0.is_finite()
                        || !p.screen.1.is_finite()
                        || p.screen.0 <= 0.0
                        || p.screen.1 <= 0.0)
                {
                    (Vec::new(), (0.0, 0.0))
                } else {
                    clean_group(peer_group(p))
                };
                let stamp = PeerStamp {
                    id: p.id.clone(),
                    generation: p.gen,
                    screen: [p.screen.0.to_bits(), p.screen.1.to_bits()],
                    monitors: p
                        .monitors
                        .iter()
                        .map(|m| (m.x, m.y, m.w, m.h, m.name.clone()))
                        .collect(),
                    side: p.side,
                    monitor: p.edge_monitor,
                    layout: group.0.iter().map(|r| rect_bits(*r)).collect(),
                };
                // 設定storeはPEERSを取らないが、getはliveロックを離してから行う。
                let draft = PeerDraft {
                    id: p.id.clone(),
                    name: crate::peer_label(p).to_string(),
                    tablet: p.id.starts_with("android-"),
                    rects: group.0,
                    size: group.1,
                    side: p.side,
                    monitor: p.edge_monitor,
                    settings: crate::screen_settings::PeerSettings {
                        source: (0.0, 1.0),
                        target: (0.0, 1.0),
                        sensitivity: 1.0,
                    },
                    original_side: p.side,
                    original_monitor: p.edge_monitor,
                    original_settings: crate::screen_settings::PeerSettings {
                        source: (0.0, 1.0),
                        target: (0.0, 1.0),
                        sensitivity: 1.0,
                    },
                };
                (stamp, draft)
            })
            .unzip()
    };
    for p in &mut peers {
        p.settings = crate::screen_settings::get(&p.id, p.side);
        p.original_settings = p.settings;
    }
    LayoutDraft {
        topology: Topology {
            preview: false,
            macs,
            peers: stamps,
        },
        macs: rects,
        mac_size: size,
        mac_name: crate::hostname_label(),
        built_in,
        selected: peers.first().map(|p| p.id.clone()),
        peers,
        drag: None,
    }
}

fn preview_draft() -> LayoutDraft {
    // この架空構成はUI_PREVIEWだけ。実接続がない時のplaceholderには使用しない。
    let (macs, mac_size) = normalize_monitors(&[
        (480.0, 1700.0, 1440.0, 900.0), // 内蔵を下、外部を上
        (0.0, 0.0, 2400.0, 1350.0),
    ]);
    let mut peers = Vec::new();
    for (id, name, tablet, side, size) in [
        (
            "android-preview",
            "TABLET-PREVIEW",
            true,
            1,
            (900.0, 1440.0),
        ),
        ("pc-preview", "DESKTOP-PREVIEW", false, 0, (2320.0, 1305.0)),
    ] {
        let mut settings = crate::screen_settings::get(id, side);
        // 未保存時にも自由境界が読み取れる長さを使う。プレビューのstoreのみ参照。
        if settings.source == (0.0, 1.0) && settings.target == (0.0, 1.0) {
            settings.source = if tablet { (0.28, 0.62) } else { (0.3, 0.64) };
            settings.target = if tablet { (0.2, 0.6) } else { (0.3, 0.64) };
        }
        peers.push(PeerDraft {
            id: id.into(),
            name: name.into(),
            tablet,
            rects: vec![(0.0, 0.0, size.0, size.1)],
            size,
            side,
            monitor: Some(1),
            settings,
            original_side: side,
            original_monitor: Some(1),
            original_settings: settings,
        });
    }
    LayoutDraft {
        topology: Topology {
            preview: true,
            macs: Vec::new(),
            peers: Vec::new(),
        },
        macs,
        mac_size,
        mac_name: "MacBook Pro".into(),
        built_in: vec![true, false],
        selected: peers.first().map(|p| p.id.clone()),
        peers,
        drag: None,
    }
}

pub(super) fn reset() {
    let previous = lock_draft().as_ref().and_then(|d| d.selected.clone());
    let mut next = runtime_draft();
    if let Some(id) = previous.filter(|id| next.peers.iter().any(|p| &p.id == id)) {
        next.selected = Some(id);
    }
    *lock_draft() = Some(next);
    unsafe {
        redraw();
    }
}

pub(super) fn dirty() -> bool {
    lock_draft()
        .as_ref()
        .is_some_and(|d| d.peers.iter().any(PeerDraft::dirty))
}

pub(super) fn choices() -> Vec<(String, String)> {
    lock_draft()
        .as_ref()
        .map(|d| {
            d.peers
                .iter()
                .map(|p| (p.id.clone(), p.name.clone()))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn placement_for_selected() -> Option<(u8, Option<usize>)> {
    let state = lock_draft();
    let d = state.as_ref()?;
    d.peers
        .iter()
        .find(|p| Some(&p.id) == d.selected.as_ref())
        .map(|p| (p.side, p.monitor))
}

pub(super) fn set_placement(side: u8, monitor: Option<usize>) -> Result<(), String> {
    let mut state = lock_draft();
    let d = state.as_mut().ok_or("編集対象がありません")?;
    if side > 7 || monitor.is_some_and(|i| i >= d.macs.len()) {
        return Err("画面または方向が不正です".into());
    }
    let selected = d.selected.clone();
    let p = d
        .peers
        .iter_mut()
        .find(|p| Some(&p.id) == selected.as_ref())
        .ok_or("端末を選択してください")?;
    p.side = side;
    p.monitor = monitor;
    d.drag = None;
    drop(state);
    notify_changed();
    Ok(())
}

pub(super) fn validate_pending() -> Result<(), String> {
    let d = lock_draft().clone().ok_or("配置の編集を開始していません")?;
    if !d.peers.iter().any(PeerDraft::dirty) {
        return Ok(());
    }
    validate(&d)?;
    if runtime_draft().topology != d.topology {
        return Err("接続または画面構成が変わりました。元に戻して再読み込みしてください".into());
    }
    Ok(())
}

pub(super) fn selected() -> Option<(String, u8)> {
    let state = lock_draft();
    let d = state.as_ref()?;
    let p = d
        .peers
        .iter()
        .find(|p| Some(&p.id) == d.selected.as_ref())?;
    Some((p.id.clone(), p.side))
}

pub(super) fn select_first() {
    let id = lock_draft()
        .as_ref()
        .and_then(|d| d.peers.first().map(|p| p.id.clone()));
    if let Some(id) = id {
        select_id(&id);
    }
}

pub(super) fn select_id(id: &str) {
    let changed = {
        let mut state = lock_draft();
        if let Some(d) = state.as_mut() {
            if d.peers.iter().any(|p| p.id == id) && d.selected.as_deref() != Some(id) {
                cancel_drag(d);
                d.selected = Some(id.to_string());
                true
            } else {
                false
            }
        } else {
            false
        }
    };
    if changed {
        notify_changed();
    }
}

pub(super) fn settings_for_selected() -> Option<crate::screen_settings::PeerSettings> {
    let state = lock_draft();
    let d = state.as_ref()?;
    d.peers
        .iter()
        .find(|p| Some(&p.id) == d.selected.as_ref())
        .map(|p| p.settings)
}

pub(super) fn set_sensitivity(value: f64) {
    if !value.is_finite() {
        return;
    }
    let changed = {
        let mut state = lock_draft();
        if let Some(d) = state.as_mut() {
            cancel_drag(d);
            let id = d.selected.clone();
            if let Some(p) = d.peers.iter_mut().find(|p| Some(&p.id) == id.as_ref()) {
                let value = value.clamp(0.2, 3.0);
                let changed = p.settings.sensitivity != value;
                p.settings.sensitivity = value;
                changed
            } else {
                false
            }
        } else {
            false
        }
    };
    if changed {
        notify_changed();
    }
}

fn valid_range(range: Point) -> bool {
    range.0.is_finite()
        && range.1.is_finite()
        && range.0 >= 0.0
        && range.1 <= 1.0
        && range.1 - range.0 >= MIN_RANGE - 1e-12
}

pub(super) fn set_ranges(source: Point, target: Point) -> Result<(), String> {
    if !valid_range(source) || !valid_range(target) {
        return Err("接続範囲は0〜1の有限値で、開始と終了の間を0.01以上にしてください".into());
    }
    let changed = {
        let mut state = lock_draft();
        let d = state.as_mut().ok_or("配置の編集を開始していません")?;
        cancel_drag(d);
        let id = d.selected.clone();
        let index = d
            .peers
            .iter()
            .position(|p| Some(&p.id) == id.as_ref())
            .ok_or("編集する端末を選んでください")?;
        let monitor = source_monitor(d, &d.peers[index]);
        let p = &mut d.peers[index];
        let changed = p.settings.source != source || p.settings.target != target;
        if changed {
            p.settings.source = source;
            p.settings.target = target;
            // 旧設定の全画面指定は、自由境界を編集した時に実際に描いた画面へ確定。
            if p.monitor.is_none() {
                p.monitor = monitor;
            }
        }
        changed
    };
    if changed {
        notify_changed();
    }
    Ok(())
}

fn validate(d: &LayoutDraft) -> Result<(), String> {
    if d.drag.is_some() {
        return Err("ドラッグを終了してから適用してください".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for p in &d.peers {
        if p.id.trim().is_empty() || !ids.insert(p.id.as_str()) {
            return Err("端末IDが空か重複しています".into());
        }
        if p.side > 7 || p.monitor.is_some_and(|i| i >= d.macs.len()) {
            return Err("対象のMac画面がありません。配置を再読み込みしてください".into());
        }
        if !valid_range(p.settings.source)
            || !valid_range(p.settings.target)
            || !p.settings.sensitivity.is_finite()
            || !(0.2..=3.0).contains(&p.settings.sensitivity)
        {
            return Err("接続範囲またはポインター感度が不正です".into());
        }
        if p.dirty() && (d.macs.is_empty() || p.rects.is_empty()) {
            return Err("画面構成を取得できない端末には適用できません".into());
        }
    }
    for (i, p) in d.peers.iter().enumerate() {
        for other in &d.peers[i + 1..] {
            if source_monitor(d, p) == source_monitor(d, other)
                && crate::base_dir(p.side) == crate::base_dir(other.side)
                && p.settings.source.0 < other.settings.source.1 - 1e-12
                && other.settings.source.0 < p.settings.source.1 - 1e-12
            {
                return Err(format!(
                    "同じMac画面の{}側で接続範囲が重なっています",
                    crate::side_label(crate::base_dir(p.side))
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn apply() -> Result<(), String> {
    let mut next = lock_draft().clone().ok_or("配置の編集を開始していません")?;
    if !next.peers.iter().any(PeerDraft::dirty) {
        return Ok(());
    }
    validate(&next)?;
    // 接続の登録/切断を直列化。PEERSの添字をset_peer_sideまで安定させる。
    let changes = crate::PEER_CHANGES
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if runtime_draft().topology != next.topology {
        return Err("接続または画面構成が変わりました。編集中の値は保持しています。元に戻して再読み込みしてください".into());
    }
    if !next.peers.iter().any(PeerDraft::dirty) {
        return Ok(());
    }
    let updates: Vec<_> = next
        .peers
        .iter()
        .map(|p| (p.id.clone(), p.settings))
        .collect();
    // 全検証→原子的な保存が成功してから、配置をruntimeへ適用。保存失敗はdraftを保持。
    let before: Vec<_> = next
        .peers
        .iter()
        .map(|p| (p.id.clone(), p.original_settings))
        .collect();
    crate::screen_settings::set_many(&updates)?;
    if !cfg!(test) && !next.topology.preview {
        let placements: Vec<_> = next
            .peers
            .iter()
            .filter(|p| p.side != p.original_side || p.monitor != p.original_monitor)
            .map(|p| (p.id.clone(), p.side, p.monitor))
            .collect();
        if let Err(e) = crate::tap::apply_peer_placements(&placements) {
            if let Err(rollback) = crate::screen_settings::set_many(&before) {
                return Err(format!("{e}。境界設定の復元にも失敗しました: {rollback}"));
            }
            return Err(e);
        }
        // side/monitorの自分自身の変更を次回Applyの世代検証に織り込む。
        for (stamp, p) in next.topology.peers.iter_mut().zip(&next.peers) {
            stamp.side = p.side;
            stamp.monitor = p.monitor;
        }
    }
    for p in &mut next.peers {
        p.accept();
    }
    drop(changes);
    *lock_draft() = Some(next);
    notify_changed();
    Ok(())
}

fn notify_changed() {
    unsafe {
        redraw();
        super::refined::layout_changed();
    }
}

fn source_monitor(d: &LayoutDraft, p: &PeerDraft) -> Option<usize> {
    p.monitor
        .filter(|i| *i < d.macs.len())
        .or_else(|| facing_index(&d.macs, crate::base_dir(p.side)))
}

fn facing_index(rects: &[Rect], dir: u8) -> Option<usize> {
    rects
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            let score = |r: &Rect| match dir {
                0 => r.0 + r.2,
                1 => -r.0,
                2 => r.1 + r.3,
                _ => -r.1,
            };
            score(a).total_cmp(&score(b))
        })
        .map(|(i, _)| i)
}

fn opposite(dir: u8) -> u8 {
    match dir {
        0 => 1,
        1 => 0,
        2 => 3,
        _ => 2,
    }
}

fn bounds(rects: &[NSRect]) -> NSRect {
    if rects.is_empty() {
        return NSRect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        };
    }
    let x = rects.iter().map(|r| r.x).fold(f64::INFINITY, f64::min);
    let y = rects.iter().map(|r| r.y).fold(f64::INFINITY, f64::min);
    let x1 = rects
        .iter()
        .map(|r| r.x + r.w)
        .fold(f64::NEG_INFINITY, f64::max);
    let y1 = rects
        .iter()
        .map(|r| r.y + r.h)
        .fold(f64::NEG_INFINITY, f64::max);
    NSRect {
        x,
        y,
        w: x1 - x,
        h: y1 - y,
    }
}

fn expanded(r: NSRect, x: f64, y: f64) -> NSRect {
    NSRect {
        x: r.x - x,
        y: r.y - y,
        w: r.w + x * 2.0,
        h: r.h + y * 2.0,
    }
}

fn intersects(a: NSRect, b: NSRect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

fn inside(r: NSRect, p: Point) -> bool {
    p.0 >= r.x && p.0 <= r.x + r.w && p.1 >= r.y && p.1 <= r.y + r.h
}

fn device_at(p: &PeerDraft, center: Point) -> Device {
    let rects: Vec<_> = p
        .rects
        .iter()
        .map(|r| NSRect {
            x: center.0 - p.size.0 / 2.0 + r.0,
            y: center.1 - p.size.1 / 2.0 + r.1,
            w: r.2,
            h: r.3,
        })
        .collect();
    Device {
        id: Some(p.id.clone()),
        bounds: bounds(&rects),
        rects,
    }
}

fn scene(d: &LayoutDraft, view: NSRect) -> Scene {
    let mac_rects: Vec<_> = d
        .macs
        .iter()
        .map(|r| NSRect {
            x: r.0,
            y: r.1,
            w: r.2,
            h: r.3,
        })
        .collect();
    let mac = Device {
        id: None,
        bounds: bounds(&mac_rects),
        rects: mac_rects,
    };
    let gap = (d.mac_size.0.max(d.mac_size.1) * 0.12).max(180.0);
    let mut devices: Vec<Device> = Vec::new();
    for p in &d.peers {
        let base = source_monitor(d, p)
            .and_then(|i| mac.rects.get(i))
            .copied()
            .unwrap_or(mac.bounds);
        let (cx, cy) = (base.x + base.w / 2.0, base.y + base.h / 2.0);
        let mut center = match p.side {
            1 => (base.x - gap - p.size.0 / 2.0, cy),
            2 => (cx, base.y + base.h + gap + p.size.1 / 2.0),
            3 => (cx, base.y - gap - p.size.1 / 2.0),
            4 => (
                base.x + base.w + gap + p.size.0 / 2.0,
                base.y + base.h * 0.75,
            ),
            5 => (
                base.x + base.w + gap + p.size.0 / 2.0,
                base.y + base.h * 0.25,
            ),
            6 => (base.x - gap - p.size.0 / 2.0, base.y + base.h * 0.75),
            7 => (base.x - gap - p.size.0 / 2.0, base.y + base.h * 0.25),
            _ => (base.x + base.w + gap + p.size.0 / 2.0, cy),
        };
        let mut device = device_at(p, center);
        // 同じ辺の複数peerも全て見せる。Macの実配置と全画面の共通scaleは変えない。
        if mac
            .rects
            .iter()
            .any(|m| intersects(expanded(device.bounds, gap * 0.1, gap * 0.1), *m))
        {
            match crate::base_dir(p.side) {
                0 => center.0 = mac.bounds.x + mac.bounds.w + gap + p.size.0 / 2.0,
                1 => center.0 = mac.bounds.x - gap - p.size.0 / 2.0,
                2 => center.1 = mac.bounds.y + mac.bounds.h + gap + p.size.1 / 2.0,
                _ => center.1 = mac.bounds.y - gap - p.size.1 / 2.0,
            }
            device = device_at(p, center);
        }
        while devices
            .iter()
            .any(|other| intersects(expanded(device.bounds, gap * 0.3, gap * 0.3), other.bounds))
        {
            if crate::base_dir(p.side) < 2 {
                center.1 += p.size.1 + gap;
            } else {
                center.0 += p.size.0 + gap;
            }
            device = device_at(p, center);
        }
        devices.push(device);
    }
    let mut all = vec![mac.bounds];
    all.extend(
        devices
            .iter()
            .filter(|p| !p.rects.is_empty())
            .map(|p| p.bounds),
    );
    let world = bounds(&all);
    // 余白はbezel/stand/名前/端点ぶん。画面そのものは単一の等方scaleで変換。
    let scale = ((view.w - 72.0).max(1.0) / world.w.max(1.0))
        .min((view.h - 100.0).max(1.0) / world.h.max(1.0));
    let ox = view.x + (view.w - world.w * scale) / 2.0 - world.x * scale;
    let oy = view.y + (view.h - world.h * scale) / 2.0 - world.y * scale;
    let transform = |mut device: Device| {
        for r in &mut device.rects {
            *r = NSRect {
                x: ox + r.x * scale,
                y: oy + r.y * scale,
                w: r.w * scale,
                h: r.h * scale,
            };
        }
        device.bounds = bounds(&device.rects);
        device
    };
    Scene {
        mac: transform(mac),
        peers: devices.into_iter().map(transform).collect(),
    }
}

fn current_scene(d: &LayoutDraft, view: NSRect) -> Scene {
    if let Some(drag) = &d.drag {
        let mut scene = drag.scene.clone();
        if matches!(drag.kind, DragKind::Device { .. }) {
            if let Some(device) = scene
                .peers
                .iter_mut()
                .find(|p| p.id.as_deref() == Some(&drag.id))
            {
                let dx = drag.center.0 - (device.bounds.x + device.bounds.w / 2.0);
                let dy = drag.center.1 - (device.bounds.y + device.bounds.h / 2.0);
                device.bounds.x += dx;
                device.bounds.y += dy;
                for r in &mut device.rects {
                    r.x += dx;
                    r.y += dy;
                }
            }
        }
        scene
    } else {
        scene(d, view)
    }
}

fn edges(d: &LayoutDraft, scene: &Scene, peer: &PeerDraft) -> Option<(Edge, Edge)> {
    let dir = crate::base_dir(peer.side);
    let source = *scene.mac.rects.get(source_monitor(d, peer)?)?;
    let target = scene
        .peers
        .iter()
        .find(|p| p.id.as_deref() == Some(&peer.id))?;
    let target_index = facing_index(&peer.rects, opposite(dir))?;
    let target = *target.rects.get(target_index)?;
    // 帯はbezel上へ6ptだけ外へ寄せ、辺に沿う長さは実画面のまま保つ。
    let bezel_edge = |mut rect: NSRect, dir| {
        match dir {
            0 => rect.x += 6.0,
            1 => rect.x -= 6.0,
            2 => rect.y += 6.0,
            _ => rect.y -= 6.0,
        }
        Edge { rect, dir }
    };
    Some((bezel_edge(source, dir), bezel_edge(target, opposite(dir))))
}

fn hit_band(edge: Edge, range: Point, p: Point) -> Option<RangePart> {
    let start = edge.point(range.0);
    let end = edge.point(range.1);
    let distance = |a: Point| (a.0 - p.0).hypot(a.1 - p.1);
    let a = distance(start);
    let b = distance(end);
    if a.min(b) <= HIT_RADIUS {
        return Some(if a <= b {
            RangePart::Start
        } else {
            RangePart::End
        });
    }
    if p.0 >= start.0.min(end.0) - 8.0
        && p.0 <= start.0.max(end.0) + 8.0
        && p.1 >= start.1.min(end.1) - 8.0
        && p.1 <= start.1.max(end.1) + 8.0
    {
        Some(RangePart::Body)
    } else {
        None
    }
}

fn range_drag(range: Point, part: RangePart, delta: f64) -> Point {
    if !delta.is_finite() || !valid_range(range) {
        return range;
    }
    match part {
        RangePart::Start => (
            (range.0 + delta).clamp(0.0, (range.1 - MIN_RANGE).max(0.0)),
            range.1,
        ),
        RangePart::End => (
            range.0,
            (range.1 + delta).clamp((range.0 + MIN_RANGE).min(1.0), 1.0),
        ),
        RangePart::Body => {
            let shift = delta.clamp(-range.0, 1.0 - range.1);
            (range.0 + shift, range.1 + shift)
        }
    }
}

fn cancel_drag(d: &mut LayoutDraft) -> bool {
    if let Some(drag) = d.drag.take() {
        if let Some(p) = d.peers.iter_mut().find(|p| p.id == drag.id) {
            *p = drag.before;
        }
        true
    } else {
        false
    }
}

fn side_for_point(rect: NSRect, p: Point) -> u8 {
    let angle = (p.1 - rect.y - rect.h / 2.0)
        .atan2(p.0 - rect.x - rect.w / 2.0)
        .to_degrees();
    if (-22.5..22.5).contains(&angle) {
        0
    } else if (22.5..67.5).contains(&angle) {
        4
    } else if (67.5..112.5).contains(&angle) {
        2
    } else if (112.5..157.5).contains(&angle) {
        6
    } else if (-67.5..-22.5).contains(&angle) {
        5
    } else if (-112.5..-67.5).contains(&angle) {
        3
    } else if (-157.5..-112.5).contains(&angle) {
        7
    } else {
        1
    }
}

fn nearest_monitor(rects: &[NSRect], p: Point) -> Option<usize> {
    rects
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let distance = |r: &NSRect| {
                let dx = (r.x - p.0).max(0.0).max(p.0 - r.x - r.w);
                let dy = (r.y - p.1).max(0.0).max(p.1 - r.y - r.h);
                dx.hypot(dy)
            };
            distance(a).total_cmp(&distance(b))
        })
        .map(|(i, _)| i)
}

unsafe fn view_bounds(view: ID) -> NSRect {
    let f: unsafe extern "C" fn(ID, SEL) -> NSRect =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(view, sel(c"bounds"))
}

unsafe extern "C" fn mouse_down(view: ID, _cmd: SEL, event: ID) {
    let location = lay_point_in_view(view, event);
    let point = (location.x, location.y);
    if !point.0.is_finite() || !point.1.is_finite() {
        return;
    }
    let rect = view_bounds(view);
    let window = msg0(view, sel(c"window"));
    if !window.is_null() {
        let focus: unsafe extern "C" fn(ID, SEL, ID) -> u8 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        focus(window, sel(c"makeFirstResponder:"), view);
    }
    let changed = {
        let mut state = lock_draft();
        let Some(d) = state.as_mut() else {
            return;
        };
        cancel_drag(d);
        let scene = scene(d, rect);
        let selected = d
            .peers
            .iter()
            .find(|p| Some(&p.id) == d.selected.as_ref())
            .cloned();
        let mut range_hit = None;
        if let Some(peer) = selected {
            if let Some((source, target)) = edges(d, &scene, &peer) {
                for (is_source, edge, range) in [
                    (true, source, peer.settings.source),
                    (false, target, peer.settings.target),
                ] {
                    if let Some(part) = hit_band(edge, range, point) {
                        range_hit = Some((
                            peer.clone(),
                            DragKind::Range {
                                source: is_source,
                                part,
                                edge,
                                start: edge.position(point),
                            },
                        ));
                        break;
                    }
                }
            }
        }
        if let Some((peer, kind)) = range_hit {
            d.drag = Some(Drag {
                id: peer.id.clone(),
                before: peer,
                kind,
                down: point,
                center: point,
                moved: false,
                scene,
            });
            false
        } else {
            // 選択中を最前面に描くのと同じ順序でhit test。Macは常に固定。
            let mut order: Vec<_> = scene.peers.iter().collect();
            order.sort_by_key(|p| p.id.as_ref() == d.selected.as_ref());
            let hit = order.into_iter().rev().find(|p| {
                p.rects
                    .iter()
                    .any(|r| inside(expanded(*r, 10.0, 16.0), point))
            });
            if let Some(device) = hit {
                let id = device.id.clone().unwrap_or_default();
                let Some(peer) = d.peers.iter().find(|p| p.id == id).cloned() else {
                    return;
                };
                let center = (
                    device.bounds.x + device.bounds.w / 2.0,
                    device.bounds.y + device.bounds.h / 2.0,
                );
                let changed = d.selected.as_deref() != Some(&id);
                d.selected = Some(id.clone());
                d.drag = Some(Drag {
                    id,
                    before: peer,
                    kind: DragKind::Device {
                        grab: (point.0 - center.0, point.1 - center.1),
                    },
                    down: point,
                    center,
                    moved: false,
                    scene,
                });
                changed
            } else {
                false
            }
        }
    };
    if changed {
        notify_changed();
    } else {
        redraw();
    }
}

unsafe extern "C" fn mouse_dragged(view: ID, _cmd: SEL, event: ID) {
    let location = lay_point_in_view(view, event);
    let point = (location.x, location.y);
    if !point.0.is_finite() || !point.1.is_finite() {
        return;
    }
    let view_rect = view_bounds(view);
    let changed = {
        let mut state = lock_draft();
        let Some(d) = state.as_mut() else {
            return;
        };
        let Some(mut drag) = d.drag.take() else {
            return;
        };
        let Some(index) = d.peers.iter().position(|p| p.id == drag.id) else {
            return;
        };
        let mut changed = false;
        if (point.0 - drag.down.0).hypot(point.1 - drag.down.1) >= 2.0 || drag.moved {
            drag.moved = true;
            match drag.kind {
                DragKind::Device { grab } => {
                    if let Some(device) = drag
                        .scene
                        .peers
                        .iter()
                        .find(|p| p.id.as_deref() == Some(&drag.id))
                    {
                        let minx = view_rect.x + device.bounds.w / 2.0 + 10.0;
                        let maxx =
                            (view_rect.x + view_rect.w - device.bounds.w / 2.0 - 10.0).max(minx);
                        let miny = view_rect.y + device.bounds.h / 2.0 + 24.0;
                        let maxy =
                            (view_rect.y + view_rect.h - device.bounds.h / 2.0 - 24.0).max(miny);
                        drag.center = (
                            (point.0 - grab.0).clamp(minx, maxx),
                            (point.1 - grab.1).clamp(miny, maxy),
                        );
                        if let Some(mi) = nearest_monitor(&drag.scene.mac.rects, drag.center) {
                            let side = side_for_point(drag.scene.mac.rects[mi], drag.center);
                            let p = &mut d.peers[index];
                            changed = p.side != side || p.monitor != Some(mi);
                            p.side = side;
                            p.monitor = Some(mi);
                        }
                    }
                }
                DragKind::Range {
                    source,
                    part,
                    edge,
                    start,
                } => {
                    let before = if source {
                        drag.before.settings.source
                    } else {
                        drag.before.settings.target
                    };
                    let range = range_drag(before, part, edge.position(point) - start);
                    let monitor = source_monitor(d, &d.peers[index]);
                    let p = &mut d.peers[index];
                    let current = if source {
                        &mut p.settings.source
                    } else {
                        &mut p.settings.target
                    };
                    changed = *current != range;
                    *current = range;
                    if p.monitor.is_none() {
                        p.monitor = monitor;
                    }
                }
            }
        }
        d.drag = Some(drag);
        changed
    };
    if changed {
        notify_changed();
    } else {
        redraw();
    }
}

unsafe extern "C" fn mouse_up(_view: ID, _cmd: SEL, _event: ID) {
    let had_drag = {
        let mut state = lock_draft();
        state.as_mut().is_some_and(|d| d.drag.take().is_some())
    };
    if had_drag {
        notify_changed();
    }
}

unsafe extern "C" fn accepts_first_responder(_view: ID, _cmd: SEL) -> u8 {
    1
}

unsafe extern "C" fn cancel_operation(_view: ID, _cmd: SEL, _sender: ID) {
    let changed = lock_draft().as_mut().is_some_and(cancel_drag);
    if changed {
        notify_changed();
    }
}

unsafe extern "C" fn key_down(view: ID, _cmd: SEL, event: ID) {
    let key: unsafe extern "C" fn(ID, SEL) -> u16 =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    match key(event, sel(c"keyCode")) {
        53 => cancel_operation(view, std::ptr::null_mut(), event),
        48 => {
            let flags = crate::msg0_isize(event, sel(c"modifierFlags")) as usize;
            let window = msg0(view, sel(c"window"));
            if !window.is_null() {
                msg1_void_id(
                    window,
                    sel(if flags & (1 << 17) != 0 {
                        c"selectPreviousKeyView:"
                    } else {
                        c"selectNextKeyView:"
                    }),
                    view,
                );
            }
        }
        _ => {} // 数値入力UIがキーボード/VoiceOverでの精密編集を提供する。
    }
}

unsafe fn refresh_accessibility(view: ID) {
    let description = {
        let state = lock_draft();
        let Some(d) = state.as_ref() else {
            return;
        };
        let mut text = format!("{}、Mac画面{}枚", d.mac_name, d.macs.len());
        for p in &d.peers {
            text.push_str(&format!(
                "。{}、ID {}、画面{}枚、{}側",
                p.name,
                p.id,
                p.rects.len(),
                crate::side_label(p.side)
            ));
            if Some(&p.id) == d.selected.as_ref() {
                text.push_str(&format!(
                    "、編集中、送出元{:.0}〜{:.0}%、送出先{:.0}〜{:.0}%、感度{:.2}",
                    p.settings.source.0 * 100.0,
                    p.settings.source.1 * 100.0,
                    p.settings.target.0 * 100.0,
                    p.settings.target.1 * 100.0,
                    p.settings.sensitivity
                ));
            }
        }
        text
    };
    // Androidの見た目の名前は隠すが、AXLabelには全端末の名前を残す。
    msg1_void_id(
        view,
        sel(c"setAccessibilityLabel:"),
        nsstring(&format!("画面配置キャンバス。{description}")),
    );
    msg1_void_id(view, sel(c"setAccessibilityValue:"), nsstring(&description));
}

fn sync_clean_runtime() -> bool {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return false;
    }
    let now = crate::now_ms();
    if now.saturating_sub(LAST_RUNTIME_SYNC.load(Ordering::Relaxed)) < 750 {
        return false;
    }
    LAST_RUNTIME_SYNC.store(now, Ordering::Relaxed);
    let can_sync = lock_draft()
        .as_ref()
        .is_none_or(|d| d.drag.is_none() && !d.peers.iter().any(PeerDraft::dirty));
    if !can_sync {
        return false;
    }
    let mut next = runtime_draft();
    let mut state = lock_draft();
    if let Some(previous) = state.as_ref() {
        if previous.drag.is_some() || previous.peers.iter().any(PeerDraft::dirty) {
            return false;
        }
        let unchanged = previous.topology == next.topology
            && previous.mac_name == next.mac_name
            && previous
                .peers
                .iter()
                .zip(&next.peers)
                .all(|(a, b)| a.name == b.name && a.settings == b.settings);
        if unchanged {
            return false;
        }
        if let Some(id) = previous
            .selected
            .as_ref()
            .filter(|id| next.peers.iter().any(|p| &p.id == *id))
        {
            next.selected = Some(id.clone());
        }
    }
    *state = Some(next);
    true
}

pub(super) unsafe fn redraw() {
    let view = CANVAS.load(Ordering::Relaxed) as ID;
    if !view.is_null() {
        let synced = sync_clean_runtime();
        refresh_accessibility(view);
        msg1_void_u8(view, sel(c"setNeedsDisplay:"), 1);
        // 毎秒の親syncから呼ばれる。未編集時だけ、接続増減を数値UIにも知らせる。
        if synced {
            super::refined::layout_changed();
        }
    }
}

/// 識別番号はキャンバス内に2秒間表示する。実ディスプレイ上へのOSDは作らない。
pub(super) unsafe fn identify() {
    let view = CANVAS.load(Ordering::Relaxed) as ID;
    if view.is_null() {
        return;
    }
    IDENTIFY_UNTIL.store(crate::now_ms() + 2000, Ordering::Relaxed);
    redraw();
    msg5_timer(
        objc_getClass(c"NSTimer".as_ptr()),
        sel(c"scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:"),
        2.05,
        view,
        sel(c"knitLayoutIdentifyExpired:"),
        std::ptr::null_mut(),
        0,
    );
}

unsafe extern "C" fn identify_expired(view: ID, _cmd: SEL, _timer: ID) {
    if crate::now_ms() >= IDENTIFY_UNTIL.load(Ordering::Relaxed) {
        msg1_void_u8(view, sel(c"setNeedsDisplay:"), 1);
    }
}

pub(super) unsafe fn make_view(parent: ID, r: NSRect) -> ID {
    if lock_draft().is_none() {
        reset();
    }
    static CLASS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let class = *CLASS.get_or_init(|| {
        let superclass = objc_getClass(c"NSView".as_ptr());
        if superclass.is_null() {
            return 0;
        }
        let class = objc_allocateClassPair(superclass, c"KnitRefinedLayoutView".as_ptr(), 0);
        if class.is_null() {
            return 0;
        }
        for (name, imp, encoding) in [
            (
                c"drawRect:",
                draw as *const () as usize,
                c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}",
            ),
            (c"mouseDown:", mouse_down as *const () as usize, c"v@:@"),
            (
                c"mouseDragged:",
                mouse_dragged as *const () as usize,
                c"v@:@",
            ),
            (c"mouseUp:", mouse_up as *const () as usize, c"v@:@"),
            (c"keyDown:", key_down as *const () as usize, c"v@:@"),
            (
                c"cancelOperation:",
                cancel_operation as *const () as usize,
                c"v@:@",
            ),
            (
                c"acceptsFirstResponder",
                accepts_first_responder as *const () as usize,
                c"B@:",
            ),
            (
                c"knitLayoutIdentifyExpired:",
                identify_expired as *const () as usize,
                c"v@:@",
            ),
        ] {
            if class_addMethod(class, sel(name), imp, encoding.as_ptr()) == 0 {
                return 0;
            }
        }
        objc_registerClassPair(class);
        class as usize
    });
    if class == 0 {
        return std::ptr::null_mut();
    }
    let init: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let view = init(msg0(class as ID, sel(c"alloc")), sel(c"initWithFrame:"), r);
    if view.is_null() {
        return view;
    }
    msg1_void_u8(view, sel(c"setAccessibilityElement:"), 1);
    msg1_void_id(view, sel(c"setAccessibilityRole:"), nsstring("AXGroup"));
    msg1_void_id(
        view,
        sel(c"setAccessibilityLabel:"),
        nsstring("画面配置キャンバス"),
    );
    msg1_void_id(view, sel(c"setAccessibilityHelp:"), nsstring("接続先の画面をクリックすると編集対象を選択します。端末をドラッグしてMac画面の辺へ配置し、青い帯の両端または中央をドラッグして接続範囲を変更できます。Escapeで直前のドラッグを取り消します。キーボードとVoiceOverでは「数値で指定」から端末を選び、範囲を編集してください。Macの画面配置は変更しません。"));
    msg1_void_i64(view, sel(c"setFocusRingType:"), 1); // NSFocusRingTypeNone。数値UIのfocusを妨げない。
    msg1_void_i64(view, sel(c"setAutoresizingMask:"), 2 | 16); // width/height sizable
    CANVAS.store(view as usize, Ordering::Relaxed);
    refresh_accessibility(view);
    msg1_void_id(parent, sel(c"addSubview:"), view);
    // alloc所有権は親へ移す。CANVASは親の存続期間だけ使う非所有参照。
    msg0_void(view, sel(c"release"));
    view
}

// CoreGraphicsのみで描画し、NSBezierPathの構造体/objc ABIには依存しない。
unsafe extern "C" {
    fn CGDisplayIsBuiltin(display: u32) -> u32;
    fn CGContextSaveGState(ctx: ID);
    fn CGContextRestoreGState(ctx: ID);
    fn CGContextSetRGBFillColor(ctx: ID, r: f64, g: f64, b: f64, a: f64);
    fn CGContextFillRect(ctx: ID, rect: NSRect);
    fn CGContextFillEllipseInRect(ctx: ID, rect: NSRect);
    fn CGContextStrokeEllipseInRect(ctx: ID, rect: NSRect);
    fn CGContextAddLineToPoint(ctx: ID, x: f64, y: f64);
    fn CGContextFillPath(ctx: ID);
    fn CGContextAddPath(ctx: ID, path: ID);
    fn CGContextClip(ctx: ID);
    fn CGContextDrawLinearGradient(
        ctx: ID,
        gradient: ID,
        start: CGPoint2,
        end: CGPoint2,
        options: u32,
    );
    fn CGPathCreateWithRoundedRect(
        rect: NSRect,
        corner_width: f64,
        corner_height: f64,
        transform: *const core::ffi::c_void,
    ) -> ID;
    fn CGPathRelease(path: ID);
    fn CGGradientCreateWithColorComponents(
        space: ID,
        components: *const f64,
        locations: *const f64,
        count: usize,
    ) -> ID;
}

unsafe fn rounded(ctx: ID, r: NSRect, radius: f64, color: (f64, f64, f64, f64)) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    let path = CGPathCreateWithRoundedRect(
        r,
        radius.min(r.w / 2.0).min(r.h / 2.0),
        radius.min(r.w / 2.0).min(r.h / 2.0),
        std::ptr::null(),
    );
    if path.is_null() {
        return;
    }
    CGContextSetRGBFillColor(ctx, color.0, color.1, color.2, color.3);
    CGContextAddPath(ctx, path);
    CGContextFillPath(ctx);
    CGPathRelease(path);
}

unsafe fn glass(ctx: ID, r: NSRect) {
    rounded(ctx, r, 2.0, (0.72, 0.74, 0.77, 1.0));
    let space = CGColorSpaceCreateDeviceRGB();
    if space.is_null() {
        return;
    }
    let components = [0.80, 0.81, 0.83, 1.0, 0.62, 0.65, 0.69, 1.0];
    let gradient =
        CGGradientCreateWithColorComponents(space, components.as_ptr(), std::ptr::null(), 2);
    if !gradient.is_null() {
        CGContextSaveGState(ctx);
        let path = CGPathCreateWithRoundedRect(r, 2.0, 2.0, std::ptr::null());
        if !path.is_null() {
            CGContextAddPath(ctx, path);
            CGContextClip(ctx);
            CGContextDrawLinearGradient(
                ctx,
                gradient,
                CGPoint2 {
                    x: r.x,
                    y: r.y + r.h,
                },
                CGPoint2 {
                    x: r.x + r.w * 0.7,
                    y: r.y,
                },
                0,
            );
            CGPathRelease(path);
        }
        CGContextRestoreGState(ctx);
        CFRelease(gradient);
    }
    CFRelease(space);
}

unsafe fn hardware(
    ctx: ID,
    r: NSRect,
    tablet: bool,
    laptop: bool,
    selected: bool,
    home: super::refined_home::Home,
) {
    let bezel = expanded(r, 6.0, 6.0);
    if selected {
        rounded(
            ctx,
            expanded(bezel, 2.5, 2.5),
            if tablet { 12.0 } else { 7.0 },
            (0.03, 0.24, 1.0, 1.0),
        );
    }
    // 控えめな接地影。色付きの機材/矢印/接続線は描かない。
    let mut shadow = expanded(bezel, 2.0, 2.0);
    shadow.y -= 3.0;
    rounded(
        ctx,
        shadow,
        if tablet { 12.0 } else { 7.0 },
        (0.2, 0.22, 0.25, 0.08),
    );
    rounded(
        ctx,
        bezel,
        if tablet { 10.0 } else { 5.0 },
        (0.29, 0.30, 0.32, 1.0),
    );
    rounded(ctx, expanded(r, 1.0, 1.0), 2.0, (0.47, 0.49, 0.51, 1.0));
    if !super::refined_home::draw(ctx, r, home) {
        glass(ctx, r);
    }
    if tablet {
        CGContextSetRGBFillColor(ctx, 0.72, 0.74, 0.77, 1.0);
        CGContextFillEllipseInRect(
            ctx,
            NSRect {
                x: r.x + r.w / 2.0 - 1.3,
                y: r.y + r.h + 1.5,
                w: 2.6,
                h: 2.6,
            },
        );
    } else if laptop {
        let base = NSRect {
            x: bezel.x - 16.0,
            y: bezel.y - 6.0,
            w: bezel.w + 32.0,
            h: 6.0,
        };
        rounded(ctx, base, 2.8, (0.62, 0.64, 0.67, 1.0));
        rounded(
            ctx,
            NSRect {
                x: base.x + 2.0,
                y: base.y + 3.0,
                w: base.w - 4.0,
                h: 2.0,
            },
            1.0,
            (0.82, 0.83, 0.85, 1.0),
        );
        rounded(
            ctx,
            NSRect {
                x: r.x + r.w * 0.4,
                y: base.y + 2.0,
                w: r.w * 0.2,
                h: 2.0,
            },
            0.8,
            (0.55, 0.57, 0.60, 1.0),
        );
        // カメラnotch。画面の矩形/比率は変えない。
        rounded(
            ctx,
            NSRect {
                x: r.x + r.w / 2.0 - 9.0,
                y: r.y + r.h - 3.0,
                w: 18.0,
                h: 4.0,
            },
            1.5,
            (0.29, 0.30, 0.32, 1.0),
        );
    } else {
        let stem_width = r.w * 0.12;
        rounded(
            ctx,
            NSRect {
                x: r.x + r.w / 2.0 - stem_width / 2.0,
                y: bezel.y - 17.0,
                w: stem_width,
                h: 18.0,
            },
            1.0,
            (0.63, 0.65, 0.68, 1.0),
        );
        rounded(
            ctx,
            NSRect {
                x: r.x + r.w * 0.28,
                y: bezel.y - 21.0,
                w: r.w * 0.44,
                h: 5.0,
            },
            2.0,
            (0.67, 0.69, 0.72, 1.0),
        );
        rounded(
            ctx,
            NSRect {
                x: r.x + r.w * 0.29,
                y: bezel.y - 18.0,
                w: r.w * 0.42,
                h: 1.0,
            },
            0.5,
            (0.87, 0.88, 0.89, 1.0),
        );
    }
}

unsafe fn draw_group(ctx: ID, device: &Device, name: Option<&str>) {
    if device.rects.is_empty() {
        return;
    }
    let b = device.bounds;
    rounded(
        ctx,
        NSRect {
            x: b.x - 24.0,
            y: b.y - 32.0,
            w: b.w + 48.0,
            h: b.h + 70.0,
        },
        13.0,
        (0.91, 0.93, 0.95, 0.36),
    );
    if let Some(name) = name {
        lay_text(
            &lay_fit(name, b.w + 35.0, 13.0),
            b.x + b.w / 2.0,
            b.y + b.h + 17.0,
            13.0,
            false,
        );
    }
}

unsafe fn draw_band(ctx: ID, edge: Edge, range: Point, selected: bool) {
    if !valid_range(range) {
        return;
    }
    let start = edge.point(range.0);
    let end = edge.point(range.1);
    CGContextSetRGBStrokeColor(
        ctx,
        if selected { 0.03 } else { 0.54 },
        if selected { 0.24 } else { 0.57 },
        if selected { 1.0 } else { 0.61 },
        if selected { 1.0 } else { 0.75 },
    );
    CGContextSetLineWidth(ctx, if selected { 6.0 } else { 4.0 });
    CGContextSetLineCap(ctx, 1);
    CGContextBeginPath(ctx);
    CGContextMoveToPoint(ctx, start.0, start.1);
    CGContextAddLineToPoint(ctx, end.0, end.1);
    CGContextStrokePath(ctx);
    if selected {
        for p in [start, end] {
            let circle = NSRect {
                x: p.0 - 6.0,
                y: p.1 - 6.0,
                w: 12.0,
                h: 12.0,
            };
            CGContextSetRGBFillColor(ctx, 1.0, 1.0, 1.0, 1.0);
            CGContextFillEllipseInRect(ctx, circle);
            CGContextSetLineWidth(ctx, 2.2);
            CGContextStrokeEllipseInRect(ctx, circle);
        }
    }
}

unsafe fn draw_number(ctx: ID, r: NSRect, number: usize) {
    let center = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    rounded(
        ctx,
        NSRect {
            x: center.0 - 18.0,
            y: center.1 - 18.0,
            w: 36.0,
            h: 36.0,
        },
        9.0,
        (0.03, 0.24, 1.0, 0.96),
    );
    lay_text(&number.to_string(), center.0, center.1 - 12.0, 20.0, true);
}

unsafe extern "C" fn draw(view: ID, _cmd: SEL, _dirty: NSRect) {
    let current = msg0(
        objc_getClass(c"NSGraphicsContext".as_ptr()),
        sel(c"currentContext"),
    );
    if current.is_null() {
        return;
    }
    let ctx = msg0(current, sel(c"CGContext"));
    if ctx.is_null() {
        return;
    }
    let r = view_bounds(view);
    let snapshot = lock_draft().clone();
    let Some(d) = snapshot else {
        return;
    };
    let scene = current_scene(&d, r);
    CGContextSaveGState(ctx);
    // 背景だけを動的システム色で塗る。グリッドや説明文は置かない。
    msg0_void(super::refined::surface_color(view, false), sel(c"setFill"));
    CGContextFillRect(ctx, r);
    draw_group(ctx, &scene.mac, Some(&d.mac_name));
    for (i, rect) in scene.mac.rects.iter().enumerate() {
        hardware(
            ctx,
            *rect,
            false,
            d.built_in.get(i).copied().unwrap_or(false),
            false,
            super::refined_home::Home::Mac,
        );
    }
    let mut order: Vec<_> = d.peers.iter().collect();
    order.sort_by_key(|p| Some(&p.id) == d.selected.as_ref());
    for p in &order {
        let Some(device) = scene
            .peers
            .iter()
            .find(|device| device.id.as_deref() == Some(&p.id))
        else {
            continue;
        };
        // 種別語は出さず、タブレットを含む全端末で端末名のみを表示する。AXValueにはIDと名前を全台分含める。
        draw_group(ctx, device, Some(&p.name));
        for rect in &device.rects {
            hardware(
                ctx,
                *rect,
                p.tablet,
                false,
                Some(&p.id) == d.selected.as_ref(),
                if p.tablet {
                    super::refined_home::Home::Android
                } else {
                    super::refined_home::Home::Windows
                },
            );
        }
    }
    // 非選択の範囲はグレー、選択中のsource/targetだけ青の4端点。
    for p in order {
        if let Some((source, target)) = edges(&d, &scene, p) {
            let active = Some(&p.id) == d.selected.as_ref();
            draw_band(ctx, source, p.settings.source, active);
            draw_band(ctx, target, p.settings.target, active);
        }
    }
    if crate::now_ms() < IDENTIFY_UNTIL.load(Ordering::Relaxed) {
        let mut number = 1;
        for rect in scene
            .mac
            .rects
            .iter()
            .chain(scene.peers.iter().flat_map(|p| p.rects.iter()))
        {
            draw_number(ctx, *rect, number);
            number += 1;
        }
    }
    CGContextRestoreGState(ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_edges_always_increase_top_down_or_left_right() {
        let rect = NSRect {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 200.0,
        };
        for dir in 0..4 {
            let edge = Edge { rect, dir };
            for value in [0.0, 0.2, 0.8, 1.0] {
                assert!((edge.position(edge.point(value)) - value).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn endpoints_and_body_clamp_without_inverting() {
        for part in [RangePart::Start, RangePart::End, RangePart::Body] {
            for delta in [-100.0, -0.2, 0.0, 0.2, 100.0, f64::NAN, f64::INFINITY] {
                let range = range_drag((0.2, 0.6), part, delta);
                assert!(valid_range(range));
                if matches!(part, RangePart::Body) {
                    assert!((range.1 - range.0 - 0.4).abs() < 1e-12);
                }
            }
        }
        assert!(!valid_range((f64::NAN, 1.0)));
        assert!(!valid_range((0.5, 0.505)));
    }

    #[test]
    fn band_has_wide_hitbox_and_draggable_body() {
        let edge = Edge {
            rect: NSRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 200.0,
            },
            dir: 0,
        };
        assert!(matches!(
            hit_band(edge, (0.2, 0.8), (108.0, 160.0)),
            Some(RangePart::Start)
        ));
        assert!(matches!(
            hit_band(edge, (0.2, 0.8), (107.0, 100.0)),
            Some(RangePart::Body)
        ));
        assert!(hit_band(edge, (0.2, 0.8), (120.0, 100.0)).is_none());
    }

    #[test]
    fn apply_validation_rejects_same_monitor_same_edge_overlap() {
        let mut d = preview_draft();
        d.peers[1].side = d.peers[0].side;
        d.peers[1].settings.source = d.peers[0].settings.source;
        assert!(validate(&d).is_err());
        d.peers[1].settings.source = (0.7, 0.9);
        assert!(validate(&d).is_ok());
        d.peers[1].settings.source = d.peers[0].settings.source;
        d.peers[1].monitor = Some(0);
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn device_drag_cancellation_restores_only_the_last_drag() {
        let mut d = preview_draft();
        d.peers[0].settings.sensitivity = 1.7; // 前に済ませた編集は取り消さない。
        let before = d.peers[0].clone();
        let scene = scene(
            &d,
            NSRect {
                x: 0.0,
                y: 0.0,
                w: 808.0,
                h: 390.0,
            },
        );
        d.drag = Some(Drag {
            id: before.id.clone(),
            before: before.clone(),
            kind: DragKind::Device { grab: (0.0, 0.0) },
            down: (0.0, 0.0),
            center: (100.0, 100.0),
            moved: true,
            scene,
        });
        d.peers[0].side = 4;
        d.peers[0].monitor = Some(0);
        d.peers[0].settings.source = (0.1, 0.3);
        assert!(cancel_drag(&mut d));
        assert_eq!(d.peers[0].side, before.side);
        assert_eq!(d.peers[0].monitor, before.monitor);
        assert_eq!(d.peers[0].settings, before.settings);
        assert!(d.peers[0].dirty());
        assert!(d.drag.is_none());
    }

    #[test]
    fn all_eight_drag_directions_are_distinct() {
        let rect = NSRect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        for (point, side) in [
            ((200.0, 50.0), 0),
            ((-100.0, 50.0), 1),
            ((50.0, 200.0), 2),
            ((50.0, -100.0), 3),
            ((200.0, 200.0), 4),
            ((200.0, -100.0), 5),
            ((-100.0, 200.0), 6),
            ((-100.0, -100.0), 7),
        ] {
            assert_eq!(side_for_point(rect, point), side);
        }
    }

    #[test]
    fn common_scale_keeps_all_screen_aspect_ratios() {
        let d = preview_draft();
        let s = scene(
            &d,
            NSRect {
                x: 0.0,
                y: 0.0,
                w: 900.0,
                h: 400.0,
            },
        );
        let scale = s.mac.rects[0].w / d.macs[0].2;
        for (raw, rect) in d.macs.iter().zip(&s.mac.rects) {
            assert!((rect.w / raw.2 - scale).abs() < 1e-12);
            assert!((rect.h / raw.3 - scale).abs() < 1e-12);
        }
        for (raw, device) in d.peers.iter().zip(&s.peers) {
            for (monitor, rect) in raw.rects.iter().zip(&device.rects) {
                assert!((rect.w / monitor.2 - scale).abs() < 1e-12);
                assert!((rect.h / monitor.3 - scale).abs() < 1e-12);
            }
        }
    }
}
