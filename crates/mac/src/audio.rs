// 音声受信・再生(Windows→Mac)。
// 独立した TCP:24901 で PCM(f32/stereo) を受け取り AudioQueue で再生する。
// 本線(24900)と分ける理由: 画像クリップの head-of-line blocking で入力が
// 止まる問題を音声で再現しないため(レビュー Wave1 H3 と同じ設計判断)
#![allow(non_snake_case)]

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

// AudioQueue の所有者は playback_worker だけ。UI/受信/Android スレッドは
// 選択世代・要求レートを更新するだけで、同期 Stop/Dispose を呼ばない。
static OUTPUT: Mutex<OutputSelection> = Mutex::new(OutputSelection {
    uid: String::new(),
    target: None,
    generation: 1,
    status: None,
});
static OUTPUT_GENERATION: AtomicU64 = AtomicU64::new(1);
static INVALIDATED_GENERATION: AtomicU64 = AtomicU64::new(0);
static OUTPUT_READY: AtomicBool = AtomicBool::new(false);
static REQUESTED_RATE: AtomicU32 = AtomicU32::new(0);
static WORKER: OnceLock<std::thread::Thread> = OnceLock::new();
static QUEUE_DISPOSE_FAILED: AtomicBool = AtomicBool::new(false);

fn output_permitted() -> bool {
    OUTPUT_READY.load(Ordering::SeqCst)
        && INVALIDATED_GENERATION.load(Ordering::SeqCst) != OUTPUT_GENERATION.load(Ordering::SeqCst)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OutputTarget {
    id: u32,
    uid: String,
    streams: Vec<u32>,
    sources: Option<Vec<u32>>,
    jack: Option<Vec<u32>>,
}

#[derive(Clone)]
struct OutputSelection {
    uid: String,
    target: Option<OutputTarget>,
    generation: u64,
    status: Option<String>,
}

impl OutputSelection {
    fn select(&mut self, uid: &str, target: OutputTarget) {
        // 0 は未失効 sentinel。世代は単調増加し、古い通知は fetch_max で排除する。
        self.generation = self.generation.saturating_add(1);
        self.uid = uid.to_string();
        self.target = Some(target);
        self.status = None;
    }

    fn restore(&mut self, uid: &str, result: Result<OutputTarget, String>) -> Result<(), String> {
        match result {
            Ok(target) => {
                self.select(uid, target);
                Ok(())
            }
            Err(reason) => {
                self.generation = self.generation.saturating_add(1);
                self.uid = uid.to_string();
                self.target = None;
                self.status = Some(reason.clone());
                Err(reason)
            }
        }
    }

    // HAL の問い合わせ中に UI が再選択していたら、古い結果を適用しない。
    // 失効は明示的な set_output_uid があるまでラッチする(再接続でも自動復帰しない)。
    fn observe(&mut self, generation: u64, current: Result<OutputTarget, String>) -> bool {
        if generation != self.generation || self.status.is_some() {
            return false;
        }
        match current {
            Ok(target) if self.target.as_ref().is_none_or(|old| old == &target) => {
                self.target = Some(target);
                false
            }
            Ok(_) => {
                self.status = Some(if self.uid.is_empty() {
                    "システム既定の出力先または物理出力が変わりました。出力先を再選択してください。"
                } else {
                    "選択した機器の物理出力または構成が変わりました。出力先を再選択してください。"
                }.to_string());
                true
            }
            Err(reason) => {
                self.status = Some(reason);
                true
            }
        }
    }
}

/// 使用可能な出力機器の (UID, 表示名)。システム既定の行は含めない。
/// UID は不透明な識別子で、同名機器を区別できる。列挙失敗時は空配列。
pub fn output_devices() -> Vec<(String, String)> {
    let Ok(ids) = property_u32s(1, address(DEVICES, GLOBAL)) else {
        return Vec::new();
    };
    let mut devices: Vec<_> = ids
        .into_iter()
        .filter_map(|id| {
            let target = device_target(id).ok()?;
            let name = property_string(id, NAME).unwrap_or_else(|_| target.uid.clone());
            Some((target.uid, name))
        })
        .collect();
    devices.sort_by(|a, b| a.0.cmp(&b.0));
    devices.dedup_by(|a, b| a.0 == b.0);
    devices.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    devices
}

/// 保存用の選択 UID。空文字列はシステム既定(その時点の実機器に固定)。
pub fn output_uid() -> String {
    OUTPUT.lock().unwrap_or_else(|e| e.into_inner()).uid.clone()
}

/// 出力先を検証して選択する。存在しない UID は旧選択を変更せず Err を返す。
/// Ok は選択の受理であり、AudioQueue の再作成完了ではない。
/// 切替時は必ず消音する。失効後の復帰には同じ UID でも再選択し、その後 UI 側で
/// 明示的に MUTED を解除する。再生開始失敗は output_status に非同期で現れる。
pub fn set_output_uid(uid: &str) -> Result<(), String> {
    let target = resolve_output(uid)?;
    {
        let mut selection = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
        OUTPUT_READY.store(false, Ordering::SeqCst);
        MUTED.store(true, Ordering::SeqCst);
        selection.select(uid, target);
        OUTPUT_GENERATION.store(selection.generation, Ordering::SeqCst);
    }
    wake_playback_worker();
    Ok(())
}

/// 起動時の保存値復元専用。失敗時も要求UIDを保持して出力を閉じ、既定へ代替しない。
/// Errは「保存先を復元したが機器は使用不能」を意味する。明示選択にはset_output_uidを使う。
pub fn restore_output_uid(uid: &str) -> Result<(), String> {
    let resolved = resolve_output(uid);
    let result = {
        let mut selection = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
        OUTPUT_READY.store(false, Ordering::SeqCst);
        MUTED.store(true, Ordering::SeqCst);
        let result = selection.restore(uid, resolved);
        OUTPUT_GENERATION.store(selection.generation, Ordering::SeqCst);
        result
    };
    wake_playback_worker();
    result
}

/// 確認音用に同期再検査した実機器UIDを返す。空の既定値をそのまま返さない。
pub fn confirmed_output_uid() -> Result<String, String> {
    let generation = OUTPUT_GENERATION.load(Ordering::SeqCst);
    check_output_topology();
    let selection = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    if selection.generation != generation || OUTPUT_GENERATION.load(Ordering::SeqCst) != generation
    {
        return Err("出力先が変更されました。もう一度確認してください".into());
    }
    if let Some(e) = &selection.status {
        return Err(e.clone());
    }
    if INVALIDATED_GENERATION.load(Ordering::SeqCst) == generation {
        return Err("出力先の再選択が必要です".into());
    }
    selection
        .target
        .as_ref()
        .map(|t| t.uid.clone())
        .filter(|s| !s.is_empty())
        .ok_or("出力先を確認できません".into())
}

/// 出力の失効・再生キュー作成失敗理由。再接続だけでは解除されない。
pub fn output_status() -> Option<String> {
    // HAL 通知が来てからワーカーが処理する短い間も、UI に失効を表示する。
    let selection = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    selection.status.clone().or_else(|| {
        (INVALIDATED_GENERATION.load(Ordering::SeqCst) == selection.generation)
            .then(|| "出力先の構成が変更されました。出力先を再選択してください。".to_string())
    })
}

/// native UI タイマーから呼べる。AudioQueue の操作は行わず、HAL の現在値を検証。
/// ワーカーも定期検証するため、設定画面を閉じても監視は継続する。
pub fn check_output_topology() {
    let before = OUTPUT.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if before.status.is_some() {
        return;
    }
    let current = if INVALIDATED_GENERATION.load(Ordering::SeqCst) == before.generation {
        Err("出力先の構成が変更されました。出力先を再選択してください。".to_string())
    } else {
        resolve_output(&before.uid)
    };
    let mut selection = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    if selection.observe(before.generation, current) {
        OUTPUT_READY.store(false, Ordering::SeqCst);
        MUTED.store(true, Ordering::SeqCst);
        if let Some(worker) = WORKER.get() {
            worker.unpark();
        }
    }
}

fn wake_playback_worker() {
    WORKER
        .get_or_init(|| {
            std::thread::Builder::new()
                .name("knit-audio-output".into())
                .spawn(playback_worker)
                .expect("audio output worker")
                .thread()
                .clone()
        })
        .unpark();
}

// HAL/AQ 通知ではロックも AudioQueue 操作も行わない。同期停止中にも呼ばれる。
fn invalidate_output(generation: u64) {
    // 遅れて完了した旧世代通知が、新世代の失効を上書きしない。
    INVALIDATED_GENERATION.fetch_max(generation, Ordering::SeqCst);
    // global gate を check→store で閉じると、その間の再選択を旧通知が消音する。
    // 即時停止は output_permitted の世代ラッチで行い、UI状態はワーカーが反映する。
    if let Some(worker) = WORKER.get() {
        worker.unpark();
    }
}

/// メニューからのミュート(受信は続くが再生しない)
pub static MUTED: AtomicBool = AtomicBool::new(false);
/// Windows 音声の再生ゲイン(受信サンプルへ掛ける倍率。1.0=等倍)。
/// f32 を atomic で保持するためビット列で持つ(0x3F800000 = 1.0f32)。
/// Mac の音量キーは Windows 操作中は Windows 側へ転送され、エンドポイント音量は
/// Windows のループバック取り出し点に効かない環境があるため、Knit 内で完結する
/// 音量調整として設定画面のスライダから変更する
static GAIN_BITS: AtomicU32 = AtomicU32::new(0x3F80_0000);

/// 現在の再生ゲイン
pub fn gain() -> f32 {
    f32::from_bits(GAIN_BITS.load(Ordering::Relaxed))
}

/// 再生ゲインを設定する(0.0..=2.0 の範囲に丸める)
pub fn set_gain(v: f64) {
    let g = if v.is_finite() {
        (v as f32).clamp(0.0, 2.0)
    } else {
        1.0
    };
    GAIN_BITS.store(g.to_bits(), Ordering::Relaxed);
}
/// 診断カウンタ(受信/再生バイト数)。10秒毎にログへ出す
static RX_BYTES: AtomicU64 = AtomicU64::new(0);
static PLAY_BYTES: AtomicU64 = AtomicU64::new(0);
/// 再生リングバッファ(音声スレッド→AudioQueue コールバック)。
/// 溢れたら古い方を捨てる。実効的な滞留は aq_callback の 2 段階クリップで
/// 「目標 ≈83ms・上限 333ms」に保つ(下記定数参照)
static RING: Mutex<std::collections::VecDeque<u8>> = Mutex::new(std::collections::VecDeque::new());
/// 第2レーン(Android タブレットの中継)。Windows 経路(ネットワーク)と
/// 同時に届くため、再生コールバックでサンプル単位に加算して「全部の音を重ねる」。
/// プリロールとドレインのみ持ち、高品質な位相追い込みは第1レーン側に任せる
static RING_B: Mutex<std::collections::VecDeque<u8>> =
    Mutex::new(std::collections::VecDeque::new());
static PRIMED_B: AtomicBool = AtomicBool::new(false);
/// 滞留の上限(≈125ms)。溢れたら古い方を捨てる(遅延をこれ以上溜めない)
const RING_CAP: usize = 48 * 1024;
/// プリロール量(48kHz f32/stereo で ≈42ms)。ストリーム開始/ミュート明けに
/// これだけ溜まるまで再生を始めない。ネットワークの到着むら(バースト)を
/// このクッションで吸収し、RING 空による断続音(モールス音)を防ぐ。
/// 低遅延を優先して 42ms とする(旧 83ms は体感の遅延が大きかった)
const RING_PRE_ROLL: usize = 16 * 1024;
/// プリロール量(再生開始までの溜め)。接続先の種別によらず同じ値を使う
static PRE_ROLL: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(RING_PRE_ROLL);
/// プリロール完了状態(開始/ミュート明けごとにやり直す)
static PRIMED: AtomicBool = AtomicBool::new(false);
/// ソフト追い込みの目標滞留(≈42ms=プリロール量と同値)。これを超えたら
/// 毎回「超過分の 1/32」だけ捨て、指数的に目標へ戻す。
/// 1 回あたりのドロップが数サンプル〜数ms 程度に収まるため、
/// まとめ捨て(位相ジャンプ=プツプツ音)にならない
const RING_SOFT_TARGET: usize = 16 * 1024;
/// 緊急クリップの上限(≈166ms)。大バースト(再接続直後等)で一気に溜まった
/// 場合だけ目標値まで一括で捨てる(恒常的には発動しない)
const RING_HARD_CLIP: usize = 64 * 1024;

type AudioQueueRef = *mut core::ffi::c_void;
type AudioQueueBufferRef = *mut AudioQueueBuffer;
type OSStatus = i32;
type CFStringRef = *const core::ffi::c_void;

// 実 SDK: CoreAudio/AudioHardware{,Base}.h, AudioToolbox/AudioQueue.h。
// AudioObjectID/selector/scope/element は UInt32、CFIndex は signed long。
#[repr(C)]
#[derive(Clone, Copy)]
struct AudioObjectPropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}
const GLOBAL: u32 = u32::from_be_bytes(*b"glob");
const OUTPUT_SCOPE: u32 = u32::from_be_bytes(*b"outp");
const DEVICES: u32 = u32::from_be_bytes(*b"dev#");
const DEFAULT_OUTPUT: u32 = u32::from_be_bytes(*b"dOut");
const DEVICE_UID: u32 = u32::from_be_bytes(*b"uid ");
const NAME: u32 = u32::from_be_bytes(*b"lnam");
const ALIVE: u32 = u32::from_be_bytes(*b"livn");
const STREAMS: u32 = u32::from_be_bytes(*b"stm#");
const DATA_SOURCE: u32 = u32::from_be_bytes(*b"ssrc");
const JACK: u32 = u32::from_be_bytes(*b"jack");
const CURRENT_DEVICE: u32 = u32::from_be_bytes(*b"aqcd");
const UTF8: u32 = 0x0800_0100;

fn address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        selector,
        scope,
        element: 0,
    }
}

type PropertyListener = unsafe extern "C" fn(
    u32,
    u32,
    *const AudioObjectPropertyAddress,
    *mut core::ffi::c_void,
) -> OSStatus;

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectHasProperty(id: u32, addr: *const AudioObjectPropertyAddress) -> u8;
    fn AudioObjectGetPropertyDataSize(
        id: u32,
        addr: *const AudioObjectPropertyAddress,
        qualifier_size: u32,
        qualifier: *const core::ffi::c_void,
        size: *mut u32,
    ) -> OSStatus;
    fn AudioObjectGetPropertyData(
        id: u32,
        addr: *const AudioObjectPropertyAddress,
        qualifier_size: u32,
        qualifier: *const core::ffi::c_void,
        size: *mut u32,
        data: *mut core::ffi::c_void,
    ) -> OSStatus;
    fn AudioObjectAddPropertyListener(
        id: u32,
        addr: *const AudioObjectPropertyAddress,
        listener: PropertyListener,
        user: *mut core::ffi::c_void,
    ) -> OSStatus;
    fn AudioObjectRemovePropertyListener(
        id: u32,
        addr: *const AudioObjectPropertyAddress,
        listener: PropertyListener,
        user: *mut core::ffi::c_void,
    ) -> OSStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringGetLength(value: CFStringRef) -> isize;
    fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    fn CFStringGetCString(value: CFStringRef, buffer: *mut i8, size: isize, encoding: u32) -> u8;
    fn CFStringCreateWithBytes(
        allocator: *const core::ffi::c_void,
        bytes: *const u8,
        length: isize,
        encoding: u32,
        external: u8,
    ) -> CFStringRef;
    fn CFRelease(value: *const core::ffi::c_void);
}

fn property_u32s(id: u32, addr: AudioObjectPropertyAddress) -> Result<Vec<u32>, String> {
    unsafe {
        // 列挙中の機器追加による size/data の競合は少数回だけ再試行する。
        for _ in 0..3 {
            let mut size = 0u32;
            let st = AudioObjectGetPropertyDataSize(id, &addr, 0, std::ptr::null(), &mut size);
            if st != 0 || size % 4 != 0 || size > 1024 * 1024 {
                return Err(format!(
                    "出力機器情報を取得できません (property={}, status={st})",
                    addr.selector
                ));
            }
            if size == 0 {
                return Ok(Vec::new());
            }
            let mut values = vec![0u32; size as usize / 4];
            let capacity = size;
            let st = AudioObjectGetPropertyData(
                id,
                &addr,
                0,
                std::ptr::null(),
                &mut size,
                values.as_mut_ptr().cast(),
            );
            if st == 0 && size <= capacity && size % 4 == 0 {
                values.truncate(size as usize / 4);
                return Ok(values);
            }
        }
    }
    Err("出力機器の構成が変更中です。もう一度選択してください。".to_string())
}

fn property_u32(id: u32, selector: u32) -> Result<u32, String> {
    let values = property_u32s(id, address(selector, GLOBAL))?;
    if values.len() != 1 {
        return Err("出力機器の単一プロパティが不正です。".to_string());
    }
    Ok(values[0])
}

unsafe fn cf_string(value: CFStringRef) -> Result<String, String> {
    unsafe {
        if value.is_null() {
            return Err("出力機器の文字列がありません。".to_string());
        }
        let size = CFStringGetMaximumSizeForEncoding(CFStringGetLength(value), UTF8);
        if !(0..1024 * 1024).contains(&size) {
            return Err("出力機器の文字列長が不正です。".to_string());
        }
        let mut bytes = vec![0u8; size as usize + 1];
        if CFStringGetCString(value, bytes.as_mut_ptr().cast(), bytes.len() as isize, UTF8) == 0 {
            return Err("出力機器の文字列を変換できません。".to_string());
        }
        let length = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        bytes.truncate(length);
        String::from_utf8(bytes)
            .map_err(|_| "出力機器の文字列が UTF-8 ではありません。".to_string())
    }
}

fn property_string(id: u32, selector: u32) -> Result<String, String> {
    unsafe {
        let mut value: CFStringRef = std::ptr::null();
        let mut size = std::mem::size_of::<CFStringRef>() as u32;
        let st = AudioObjectGetPropertyData(
            id,
            &address(selector, GLOBAL),
            0,
            std::ptr::null(),
            &mut size,
            (&mut value as *mut CFStringRef).cast(),
        );
        if st != 0 {
            return Err(format!("出力機器の名称/UIDを取得できません (status={st})"));
        }
        // HAL の UID/Name は Get でも所有権が渡る(実 SDK の契約)。
        let result = cf_string(value);
        if !value.is_null() {
            CFRelease(value);
        }
        result.and_then(|text| {
            if size as usize == std::mem::size_of::<CFStringRef>() {
                Ok(text)
            } else {
                Err("出力機器の文字列プロパティ長が不正です。".to_string())
            }
        })
    }
}

fn device_target(id: u32) -> Result<OutputTarget, String> {
    if id == 0 || property_u32(id, ALIVE)? != 1 {
        return Err("選択した出力機器が利用できません。出力先を再選択してください。".to_string());
    }
    let streams = property_u32s(id, address(STREAMS, OUTPUT_SCOPE))?;
    if streams.is_empty() {
        return Err("選択した機器には出力ストリームがありません。".to_string());
    }
    let uid = property_string(id, DEVICE_UID)?;
    if uid.is_empty() || uid.contains('\0') {
        return Err("出力機器の UID が不正です。".to_string());
    }
    Ok(OutputTarget {
        id,
        uid,
        streams,
        // 内蔵イヤホン/スピーカーのように UID が同じまま物理出力だけ変わる場合も監視。
        sources: property_u32s(id, address(DATA_SOURCE, OUTPUT_SCOPE)).ok(),
        jack: property_u32s(id, address(JACK, OUTPUT_SCOPE)).ok(),
    })
}

fn resolve_output(uid: &str) -> Result<OutputTarget, String> {
    if uid.is_empty() {
        return device_target(property_u32(1, DEFAULT_OUTPUT)?);
    }
    if uid.contains('\0') {
        return Err("出力機器の UID に NUL は指定できません。".to_string());
    }
    let ids = property_u32s(1, address(DEVICES, GLOBAL))?;
    for id in ids {
        if property_string(id, DEVICE_UID).as_deref() == Ok(uid) {
            return device_target(id);
        }
    }
    Err("選択した出力機器が見つかりません。接続を確認して出力先を再選択してください。".to_string())
}

// HAL の user は非参照の整数 token。解除失敗・解除前に配送済みの通知でも
// 解放済み context に触れず、登録時の世代だけを失効できる(対象は64-bit macOS)。
fn topology_user(generation: u64) -> *mut core::ffi::c_void {
    std::ptr::without_provenance_mut(generation as usize)
}

fn topology_generation(user: *mut core::ffi::c_void) -> Option<u64> {
    let generation = user.addr() as u64;
    (generation != 0).then_some(generation)
}

unsafe extern "C" fn topology_changed(
    _id: u32,
    _count: u32,
    _addresses: *const AudioObjectPropertyAddress,
    user: *mut core::ffi::c_void,
) -> OSStatus {
    if let Some(generation) = topology_generation(user) {
        invalidate_output(generation);
    }
    0
}

// NewOutput 前から Dispose 成功後まで同じアドレスで保持する。
// 世代は不変。通知・再投入ゲートも global でなくこの AQ だけに所属する。
struct QueueContext {
    generation: u64,
    queue: AtomicPtr<core::ffi::c_void>,
    callbacks: AtomicBool,
    watching: AtomicBool,
    device_changes: AtomicU64,
    verified_changes: AtomicU64,
}

impl QueueContext {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            queue: AtomicPtr::new(std::ptr::null_mut()),
            callbacks: AtomicBool::new(false),
            watching: AtomicBool::new(false),
            device_changes: AtomicU64::new(0),
            verified_changes: AtomicU64::new(0),
        }
    }

    fn matches(&self, aq: AudioQueueRef) -> bool {
        !aq.is_null() && self.queue.load(Ordering::SeqCst) == aq
    }

    fn notify_device_change(&self, aq: AudioQueueRef, property: u32) -> bool {
        if property != CURRENT_DEVICE || !self.matches(aq) || !self.watching.load(Ordering::SeqCst)
        {
            return false;
        }
        // 通知自体は自己 SetProperty/Start の遅延通知でも発生しうる。
        // 恒久ラッチせず即時消音し、ワーカーで実 UID を検証してから開く。
        self.device_changes.fetch_add(1, Ordering::SeqCst);
        true
    }

    fn device_verified(&self) -> bool {
        self.device_changes.load(Ordering::SeqCst) == self.verified_changes.load(Ordering::SeqCst)
    }

    fn confirm_device(
        &self,
        observed: u64,
        expected: &str,
        current: Result<String, String>,
    ) -> Result<(), String> {
        if current? != expected {
            return Err(
                "再生キューの出力先が変わりました。出力先を再選択してください。".to_string(),
            );
        }
        self.verified_changes.store(observed, Ordering::SeqCst);
        Ok(())
    }

    fn output_permitted(&self) -> bool {
        OUTPUT_GENERATION.load(Ordering::SeqCst) == self.generation
            && self.device_verified()
            && output_permitted()
    }

    fn disable(&self) {
        self.callbacks.store(false, Ordering::SeqCst);
        self.watching.store(false, Ordering::SeqCst);
    }
}

struct PlaybackQueue {
    aq: AudioQueueRef,
    context: Box<QueueContext>,
    uid: String,
    // AQ の停止・破棄まで登録を保持する。通知 token 自体には解放対象がない。
    _listeners: TopologyListeners,
}

unsafe extern "C" fn queue_device_changed(
    user: *mut core::ffi::c_void,
    aq: AudioQueueRef,
    property: u32,
) {
    // context の寿命は所有ワーカーの同期 Dispose により保証する。
    if let Some(context) = unsafe { user.cast::<QueueContext>().as_ref() } {
        if context.notify_device_change(aq, property) {
            if let Some(worker) = WORKER.get() {
                worker.unpark();
            }
        }
    }
}

struct TopologyListeners {
    generation: u64,
    properties: Vec<(u32, AudioObjectPropertyAddress)>,
}
impl TopologyListeners {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            properties: Vec::new(),
        }
    }

    fn add(
        &mut self,
        id: u32,
        addr: AudioObjectPropertyAddress,
        required: bool,
    ) -> Result<(), String> {
        unsafe {
            if !required && AudioObjectHasProperty(id, &addr) == 0 {
                return Ok(());
            }
            let st = AudioObjectAddPropertyListener(
                id,
                &addr,
                topology_changed,
                topology_user(self.generation),
            );
            if st != 0 {
                return Err(format!("出力先変更の監視を開始できません (status={st})"));
            }
        }
        self.properties.push((id, addr));
        Ok(())
    }
}
impl Drop for TopologyListeners {
    fn drop(&mut self) {
        for (id, addr) in &self.properties {
            let st = unsafe {
                AudioObjectRemovePropertyListener(
                    *id,
                    addr,
                    topology_changed,
                    topology_user(self.generation),
                )
            };
            if st != 0 {
                // user は整数 token のため残ったリスナーから UAF は起きない。
                eprintln!("[audio] 出力先監視の解除に失敗 (device={id}, status={st})");
            }
        }
    }
}

// AudioToolbox/AudioQueue.h の現行レイアウト(タイムスタンプ等は無い。
// 旧レイアウトを仮定すると byteSize の書き込み位置がずれ BufferEmpty になる)
#[repr(C)]
struct AudioQueueBuffer {
    mAudioDataBytesCapacity: u32,
    _pad0: u32,
    mAudioData: *mut u8,
    mAudioDataByteSize: u32,
    _pad1: u32,
    mUserData: *mut core::ffi::c_void,
    mPacketDescriptionCapacity: u32,
    _pad2: u32,
    mPacketDescriptions: *mut core::ffi::c_void,
    mPacketDescriptionCount: u32,
    _pad3: u32,
}

/// LinearPCM f32 / interleaved / stereo
#[repr(C)]
#[derive(Clone, Copy)]
struct AudioStreamBasicDescription {
    mSampleRate: f64,
    mFormatID: u32,
    mFormatFlags: u32,
    mBytesPerPacket: u32,
    mFramesPerPacket: u32,
    mBytesPerFrame: u32,
    mChannelsPerFrame: u32,
    mBitsPerChannel: u32,
    mReserved: u32,
}
const KAUDIO_FORMAT_LINEAR_PCM: u32 = 0x6C_70_63_6D; // 'lpcm'
const KLINEAR_FLAGS_FLOAT_PACKED: u32 = 0x1 | (1 << 3); // IsFloat | IsPacked

#[link(name = "AudioToolbox", kind = "framework")]
#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioQueueNewOutput(
        in_desc: *const AudioStreamBasicDescription,
        in_callback_proc: unsafe extern "C" fn(
            in_user_data: *mut core::ffi::c_void,
            in_aq: AudioQueueRef,
            in_buffer: AudioQueueBufferRef,
        ),
        in_user_data: *mut core::ffi::c_void,
        in_callback_run_loop: *mut core::ffi::c_void,
        in_callback_run_loop_mode: *mut core::ffi::c_void,
        in_flags: u32,
        out_aq: *mut AudioQueueRef,
    ) -> OSStatus;
    fn AudioQueueAllocateBuffer(
        in_aq: AudioQueueRef,
        in_buffer_byte_size: u32,
        out_buffer: *mut AudioQueueBufferRef,
    ) -> OSStatus;
    fn AudioQueueEnqueueBuffer(
        in_aq: AudioQueueRef,
        in_buffer: AudioQueueBufferRef,
        in_num_packet_descs: u32,
        in_packet_descs: *const core::ffi::c_void,
    ) -> OSStatus;
    fn AudioQueueGetProperty(
        aq: AudioQueueRef,
        property: u32,
        data: *mut core::ffi::c_void,
        size: *mut u32,
    ) -> OSStatus;
    fn AudioQueueSetProperty(
        aq: AudioQueueRef,
        property: u32,
        data: *const core::ffi::c_void,
        size: u32,
    ) -> OSStatus;
    fn AudioQueueAddPropertyListener(
        aq: AudioQueueRef,
        property: u32,
        callback: unsafe extern "C" fn(*mut core::ffi::c_void, AudioQueueRef, u32),
        user: *mut core::ffi::c_void,
    ) -> OSStatus;
    fn AudioQueueStart(in_aq: AudioQueueRef, in_time: *const core::ffi::c_void) -> OSStatus;
    fn AudioQueueStop(in_aq: AudioQueueRef, immediate: u8) -> OSStatus;
    fn AudioQueueDispose(in_aq: AudioQueueRef, immediate: u8) -> OSStatus;
}

fn ensure_playback(rate: u32) {
    if !(4000..=192_000).contains(&rate) {
        return;
    }
    let previous = REQUESTED_RATE.swap(rate, Ordering::SeqCst);
    // レート変更はワーカーでキューを停止してからゲートを閉じる。
    // ここで閉じると、短時間に A→B→A と戻った要求で開き直せなくなる。
    if previous != rate || WORKER.get().is_none() {
        wake_playback_worker();
    }
}

fn reset_rings() {
    // 必ず Stop/Dispose が返ってから、各ロックを単独取得する。
    RING.lock().unwrap_or_else(|e| e.into_inner()).clear();
    RING_B.lock().unwrap_or_else(|e| e.into_inner()).clear();
    PRIMED.store(false, Ordering::Relaxed);
    PRIMED_B.store(false, Ordering::Relaxed);
    phase_store(0.0);
}

fn fail_output(generation: u64, reason: String) {
    let mut selection = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    if selection.generation == generation {
        OUTPUT_READY.store(false, Ordering::SeqCst);
        MUTED.store(true, Ordering::SeqCst);
        selection.status = Some(reason);
    }
}

// 停止/破棄はこのワーカーだけが行う。raw AQ ポインタを UI/Android と共有しない。
// AQStop(true) は同期コールバックを呼ぶため、この関数では共有ロックを保持しない。
fn stop_playback(queue: PlaybackQueue) -> Result<(), String> {
    OUTPUT_READY.store(false, Ordering::SeqCst);
    queue.context.disable();
    unsafe {
        let stop = AudioQueueStop(queue.aq, 1);
        let dispose = AudioQueueDispose(queue.aq, 1);
        if dispose != 0 {
            QUEUE_DISPOSE_FAILED.store(true, Ordering::SeqCst);
            // 解放失敗時は callback がまだ来る可能性がある。context を解放しない。
            // fatal latch により新しい AQ も作らない。
            std::mem::forget(queue);
            return Err(format!(
                "再生キューを解放できません。Knit を再起動してください (stop={stop}, dispose={dispose})"
            ));
        }
        if stop != 0 {
            eprintln!("[audio] AudioQueueStop status={stop} (Dispose成功)");
        }
    }
    // 所有ワーカーから呼ぶ同期 Dispose は、復帰後の callback 不在を保証する。
    // ここで初めて Box<QueueContext> を解放できる。
    Ok(())
}

fn verify_queue_device(queue: &PlaybackQueue) -> Result<(), String> {
    // 読取中に来た通知を取りこぼさないよう、読取前の counter だけを確認済みにする。
    let observed = queue.context.device_changes.load(Ordering::SeqCst);
    unsafe {
        let mut device: CFStringRef = std::ptr::null();
        let mut size = std::mem::size_of::<CFStringRef>() as u32;
        let st = AudioQueueGetProperty(
            queue.aq,
            CURRENT_DEVICE,
            (&mut device as *mut CFStringRef).cast(),
            &mut size,
        );
        if st != 0 {
            return Err(format!("再生キューの出力先を確認できません (status={st})"));
        }
        let current = cf_string(device);
        // AQ の Get も CF オブジェクトは複製され、呼出し側が解放する契約。
        if !device.is_null() {
            CFRelease(device);
        }
        let current = if size as usize == std::mem::size_of::<CFStringRef>() {
            current
        } else {
            Err("再生キューの出力先プロパティ長が不正です。".to_string())
        };
        queue.context.confirm_device(observed, &queue.uid, current)
    }
}

fn playback_worker() {
    let mut aq: Option<PlaybackQueue> = None;
    let mut active_generation = 0;
    let mut active_rate = 0;
    // Dispose 失敗後は同一プロセス内で別キューを作らない(二重再生の可能性を避ける)。
    let mut fatal = None;
    loop {
        check_output_topology();
        let selection = OUTPUT.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let rate = REQUESTED_RATE.load(Ordering::SeqCst);
        let invalid = selection.status.is_some()
            || INVALIDATED_GENERATION.load(Ordering::SeqCst) == selection.generation;
        let changed = selection.generation != active_generation || rate != active_rate;
        if aq.is_some() && (changed || invalid) {
            if let Err(reason) = stop_playback(aq.take().unwrap()) {
                fatal = Some(reason);
            }
            reset_rings();
        }
        if QUEUE_DISPOSE_FAILED.load(Ordering::SeqCst) {
            fatal = Some(
                "再生キューの解放に失敗したため停止中です。Knit を再起動してください。".to_string(),
            );
        }
        if let Some(reason) = fatal.as_ref() {
            fail_output(selection.generation, reason.clone());
        } else if aq.is_none() && !invalid && rate != 0 {
            if let Some(target) = selection.target.as_ref() {
                reset_rings();
                // システム通知も登録世代に固定する。DEVICES はポーリング検証で扱い、
                // 既定変更は「既定」選択時だけこの世代を失効させる。
                let mut listeners = TopologyListeners::new(selection.generation);
                let watched = if selection.uid.is_empty() {
                    listeners.add(1, address(DEFAULT_OUTPUT, GLOBAL), true)
                } else {
                    Ok(())
                }
                .and_then(|_| listeners.add(target.id, address(ALIVE, GLOBAL), true))
                .and_then(|_| listeners.add(target.id, address(STREAMS, OUTPUT_SCOPE), true))
                .and_then(|_| listeners.add(target.id, address(DATA_SOURCE, OUTPUT_SCOPE), false))
                .and_then(|_| listeners.add(target.id, address(JACK, OUTPUT_SCOPE), false));
                let created = watched.and_then(|_| start_playback(rate, &target.uid, listeners));
                match created {
                    Ok(queue) => {
                        aq = Some(queue);
                        active_generation = selection.generation;
                        active_rate = rate;
                        // リスナー登録/キュー作成中の抜き差し・UI再選択も公開前に検出。
                        check_output_topology();
                    }
                    Err(reason) => {
                        fail_output(selection.generation, reason);
                    }
                }
            }
        }
        // キュー作成中のレート要求が A→B→A と戻っても、現キューが有効なら開く。
        if let Some(queue) = aq.as_ref() {
            // 通知は即時にこの AQ を消音。HAL/AQ 呼出しは callback でなく所有者だけ。
            // 同 UID の自己通知なら再開、別 UID/問い合わせ失敗なら明示的再選択が必要。
            if !queue.context.device_verified() {
                if let Err(reason) = verify_queue_device(queue) {
                    invalidate_output(queue.context.generation);
                    fail_output(queue.context.generation, reason);
                }
            }
            let current = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
            if current.generation == active_generation
                && current.status.is_none()
                && INVALIDATED_GENERATION.load(Ordering::SeqCst) != active_generation
                && REQUESTED_RATE.load(Ordering::SeqCst) == active_rate
            {
                OUTPUT_READY.store(true, Ordering::SeqCst);
            }
        }
        // 受信が止まっても切替・切断を処理する。UI タイマーは補助であり必須ではない。
        std::thread::park_timeout(std::time::Duration::from_millis(100));
    }
}

/// リサンプル位相: 前回コールバックの消費端数(フレーム単位、0..1)。
/// コールバック間で持ち越すことで、読み出し位置が波形上で完全に連続になり
/// フレーム欠け/重複が構造的に起きない(端数の切り捨てが毎回ノイズになるのを防ぐ)
static PHASE_FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn phase_load() -> f64 {
    f64::from_bits(PHASE_FRAMES.load(Ordering::Relaxed))
}
fn phase_store(v: f64) {
    PHASE_FRAMES.store(v.to_bits(), Ordering::Relaxed);
}

/// AudioQueue の出力コールバック: リングから必要分だけ取り出して埋める。
/// 不足分は無音で埋める。滞留が目標を超えているときは線形補間でわずかに
/// 速く読み(リサンプル追い込み)、波形を切らないまま遅延を調整する
static CB_FIRED: AtomicBool = AtomicBool::new(false);
/// 間引き(緊急クリップ)した累計バイト。音割れ調査の指標として diag へ出す
static DROP_BYTES: AtomicU64 = AtomicU64::new(0);
unsafe extern "C" fn aq_callback(
    user: *mut core::ffi::c_void,
    aq: AudioQueueRef,
    buffer: AudioQueueBufferRef,
) {
    unsafe {
        // Stop(true) が呼ぶ完了通知では再投入しない。停止側はリングをロックしない。
        let Some(context) = user.cast::<QueueContext>().as_ref() else {
            return;
        };
        if !context.matches(aq) || !context.callbacks.load(Ordering::SeqCst) {
            return;
        }
        if !CB_FIRED.swap(true, Ordering::Relaxed) {
            eprintln!(
                "[audio] callback alive (cap={})",
                (*buffer).mAudioDataBytesCapacity
            );
        }
        let cap = (*buffer).mAudioDataBytesCapacity as usize;
        let dst = (*buffer).mAudioData;
        let mut filled = 0usize;
        if !MUTED.load(Ordering::Relaxed) && context.output_permitted() {
            let mut ring = RING.lock().unwrap_or_else(|e| e.into_inner());
            // プリロール: RING_PRE_ROLL 分溜まるまで再生を始めない(ゼロ埋め)。
            // 溜まりきる前に鳴らし始めると供給の到着むらがそのまま音切れに
            // なる(断続音=モールス音の原因)
            if !PRIMED.load(Ordering::Relaxed) {
                let pre_roll = PRE_ROLL.load(Ordering::Relaxed);
                // 第1レーン(ネットワーク)にまだデータが無い場合は素通りする:
                // ここで return すると第2レーン(タブレット)の音まで止まってしまう
                if !ring.is_empty() && ring.len() < pre_roll {
                    std::ptr::write_bytes(dst, 0, cap);
                    (*buffer).mAudioDataByteSize = cap as u32;
                    PLAY_BYTES.fetch_add(cap as u64, Ordering::Relaxed);
                    drop(ring);
                    if context.callbacks.load(Ordering::SeqCst) {
                        AudioQueueEnqueueBuffer(aq, buffer, 0, std::ptr::null());
                    }
                    return;
                }
                if !ring.is_empty() {
                    PRIMED.store(true, Ordering::Relaxed);
                    phase_store(0.0); // 在庫が新鮮なため位相もリセット
                }
            }
            // 緊急クリップ(大バーストのみ): 8 バイト(f32×2ch=1フレーム)境界で
            // 一括間引き。超過は通常この経路を通らず、通った量は diag で見える
            if ring.len() > RING_HARD_CLIP {
                let excess = (ring.len() - RING_SOFT_TARGET) & !7usize;
                DROP_BYTES.fetch_add(excess as u64, Ordering::Relaxed);
                ring.drain(..excess);
            }
            // 滑らかな追い込み: 滞留が「目標+デッドバンド(≈5ms)」を超えている
            // 間だけ読み出し速度を最大 1% 速くする。フレーム欠けではなく波形を
            // 連続したまま遅延を目標へ戻す。位相はコールバック間で持ち越すため
            // ratio=1.0 のときは素通り=ドロップ・重複ゼロ。
            // 音質優先の設計: (a)超過が僅か(デッドバンド内)の間は補間しない
            // (b)補間は 3次 Catmull-Rom(高域までほぼフラット。線形補間は
            // 高域が減衰する)(c)速度変化の上限は 1%(知覚不可)
            const SOFT_DEAD: usize = 2048;
            let over = ring.len().saturating_sub(RING_SOFT_TARGET + SOFT_DEAD);
            let ratio = if over == 0 {
                1.0
            } else {
                1.0 + (over as f64 / RING_SOFT_TARGET as f64).min(1.0) * 0.01
            };
            // リアルタイムスレッドでメモリを確保しないよう、リングを直接読む
            let need = (cap as f64 * ratio) as usize + 16;
            let src_frames = need.min(ring.len()) / 8; // 1フレーム=8バイト(L+R)
            let out_frames = cap / 8;
            if src_frames >= 2 {
                let dst_f = dst as *mut f32;
                let sample = |i: usize| -> f32 {
                    f32::from_le_bytes([
                        ring[i * 4],
                        ring[i * 4 + 1],
                        ring[i * 4 + 2],
                        ring[i * 4 + 3],
                    ])
                };
                // sp はフレーム単位の読み出し位置(整数部=フレーム、端数=補間位相)。
                // L(偶数サンプル)は L 同士、R は R 同士で補間するため L/R は混ざらない。
                // 補間カーネルは Catmull-Rom(4点3次): 線形補間と違い高域まで
                // ほぼ劣化させない(音質優先)。frac=0 のときは厳密に素通り
                let mut sp = phase_load();
                let mut written_bytes = 0usize;
                let fidx =
                    |k: i64| -> usize { (k.max(0) as usize).min(src_frames.saturating_sub(1)) };
                for o in 0..out_frames {
                    let i0 = sp as usize;
                    if i0 + 1 >= src_frames {
                        // 在庫の終端に到達: 最終フレームをホールドして埋める
                        let base = (src_frames - 1) * 2;
                        dst_f.add(o * 2).write(sample(base));
                        dst_f.add(o * 2 + 1).write(sample(base + 1));
                        written_bytes = (o + 1) * 8;
                        sp = (src_frames - 1) as f64;
                        break;
                    }
                    let t = (sp - i0 as f64) as f32;
                    let (l, r);
                    if i0 + 2 < src_frames {
                        let t2 = t * t;
                        let t3 = t2 * t;
                        let cm = |p0: f32, p1: f32, p2: f32, p3: f32| -> f32 {
                            0.5 * ((2.0 * p1)
                                + (-p0 + p2) * t
                                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                                + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
                        };
                        let m = i0 as i64;
                        l = cm(
                            sample(fidx(m - 1) * 2),
                            sample(i0 * 2),
                            sample((i0 + 1) * 2),
                            sample(fidx(m + 2) * 2),
                        );
                        r = cm(
                            sample(fidx(m - 1) * 2 + 1),
                            sample(i0 * 2 + 1),
                            sample((i0 + 1) * 2 + 1),
                            sample(fidx(m + 2) * 2 + 1),
                        );
                    } else {
                        // 在庫の終端1フレーム手前: 線形補間でフォールバック
                        l = sample(i0 * 2) + (sample(i0 * 2 + 2) - sample(i0 * 2)) * t;
                        r = sample(i0 * 2 + 1) + (sample(i0 * 2 + 3) - sample(i0 * 2 + 1)) * t;
                    }
                    dst_f.add(o * 2).write(l);
                    dst_f.add(o * 2 + 1).write(r);
                    written_bytes = (o + 1) * 8;
                    sp += ratio;
                }
                // 消費 = 読み出し位置の整数部(フレーム単位=自動的に 8 バイト境界)。
                // 端数は PHASE として次回へ繰り越すため、捨てても重複しても無い
                let consumed_frames = (sp as usize).min(src_frames);
                let consumed = consumed_frames * 8;
                let n = consumed.min(ring.len());
                ring.drain(..n);
                phase_store(sp - consumed_frames as f64);
                filled = written_bytes;
            } else if !ring.is_empty() {
                // 在庫が 1 フレーム未満(通常ない): そのまま取り出す
                let take = ring.len().min(cap);
                for i in 0..take {
                    dst.add(i).write(ring.pop_front().unwrap_or(0));
                }
                filled = take;
            }
        }
        // 第2レーン(Android タブレット)を加算して「全部の音を重ねる」。
        // 同じ 48kHz のためサンプル位置は 1:1 で足せる。加算はソフト飽和付き。
        // 重要: 加算の前に未書き込み領域をゼロクリアする。バッファには前回の
        // 内容が残っており、そのまま足すと「前回の音+今回の音」が混ざって
        // 不連続な波形(音割れ)になる
        if !MUTED.load(Ordering::Relaxed) && context.output_permitted() {
            let mut rb = RING_B.lock().unwrap_or_else(|e| e.into_inner());
            if filled < cap {
                std::ptr::write_bytes(dst.add(filled), 0, cap - filled);
            }
            if !rb.is_empty() {
                if !PRIMED_B.load(Ordering::Relaxed) && rb.len() >= RING_PRE_ROLL {
                    PRIMED_B.store(true, Ordering::Relaxed);
                }
                if PRIMED_B.load(Ordering::Relaxed) {
                    // 超過分を 1/32 ずつ捨てて目標滞留へ戻す(荒いが第2レーン限定)
                    const SOFT_B: usize = RING_PRE_ROLL + 2048;
                    if rb.len() > SOFT_B {
                        let excess = ((rb.len() - RING_PRE_ROLL) / 32) & !7usize;
                        let n = excess.min(rb.len());
                        DROP_BYTES.fetch_add(n as u64, Ordering::Relaxed);
                        rb.drain(..n);
                    }
                    let take = (rb.len().min(cap)) & !3usize;
                    let dst_f = dst as *mut f32;
                    for i in 0..take / 4 {
                        let s = f32::from_le_bytes([
                            rb[i * 4],
                            rb[i * 4 + 1],
                            rb[i * 4 + 2],
                            rb[i * 4 + 3],
                        ]);
                        let cur = dst_f.add(i).read();
                        // ソフト飽和: 0.7 までは素通し、超えた分は滑らかに抑える。
                        // 単純な clamp は波形を平らに切って強い歪み(音割れ)になる
                        let sum = cur + s;
                        let out = if sum > 0.7 {
                            0.7 + (sum - 0.7) / (1.0 + (sum - 0.7) * 4.0) * 0.3
                        } else if sum < -0.7 {
                            -0.7 - (-sum - 0.7) / (1.0 + (-sum - 0.7) * 4.0) * 0.3
                        } else {
                            sum
                        };
                        dst_f.add(i).write(out);
                    }
                    rb.drain(..take);
                    if take > filled {
                        filled = take;
                    }
                }
            }
        }
        // 残りは無音のまま(バッファは前回の内容が残るため明示的にゼロクリア)
        if filled < cap {
            std::ptr::write_bytes(dst.add(filled), 0, cap - filled);
        }
        // HAL 通知/UI 切替がミックス中に来ても、未投入バッファは無音にする。
        if MUTED.load(Ordering::SeqCst) || !context.output_permitted() {
            std::ptr::write_bytes(dst, 0, cap);
        }
        (*buffer).mAudioDataByteSize = cap as u32;
        PLAY_BYTES.fetch_add(cap as u64, Ordering::Relaxed);
        if context.callbacks.load(Ordering::SeqCst) {
            AudioQueueEnqueueBuffer(aq, buffer, 0, std::ptr::null());
        }
    }
}

/// AudioQueue を起動する(4バッファ x 15ms = 60ms の再生バッファ)。
/// 低遅延と音切れ防止のバランス点: 供給側の揺らぎ(TCP 到着間隔)を
/// この余裕で吸収し、滞留はクリップで「目標 ≈78ms」に保つ
fn start_playback(
    rate: u32,
    uid: &str,
    listeners: TopologyListeners,
) -> Result<PlaybackQueue, String> {
    unsafe {
        let bytes_per_frame: u32 = 8; // f32 x 2ch
                                      // 15ms 分。奇数サンプルレート環境で L/R が割れないよう 8 バイト境界へ丸める
        let frame_bytes = (rate as usize * bytes_per_frame as usize * 15 / 1000) & !7usize;
        let desc = AudioStreamBasicDescription {
            mSampleRate: rate as f64,
            mFormatID: KAUDIO_FORMAT_LINEAR_PCM,
            mFormatFlags: KLINEAR_FLAGS_FLOAT_PACKED,
            mBytesPerPacket: bytes_per_frame,
            mFramesPerPacket: 1,
            mBytesPerFrame: bytes_per_frame,
            mChannelsPerFrame: 2,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        let mut context = Box::new(QueueContext::new(listeners.generation));
        let user = (&mut *context as *mut QueueContext).cast();
        let mut aq: AudioQueueRef = std::ptr::null_mut();
        let st = AudioQueueNewOutput(
            &desc,
            aq_callback,
            user,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut aq,
        );
        if st != 0 || aq.is_null() {
            // 部分的にキューが返された場合も Dispose まで context を保持する。
            if !aq.is_null() {
                context.queue.store(aq, Ordering::SeqCst);
                stop_playback(PlaybackQueue {
                    aq,
                    context,
                    uid: uid.to_string(),
                    _listeners: listeners,
                })?;
            }
            return Err(format!("再生キューを作成できません (status={st})"));
        }
        context.queue.store(aq, Ordering::SeqCst);
        let queue = PlaybackQueue {
            aq,
            context,
            uid: uid.to_string(),
            _listeners: listeners,
        };
        let setup = (|| -> Result<(), String> {
            let device = CFStringCreateWithBytes(
                std::ptr::null(),
                uid.as_ptr(),
                uid.len() as isize,
                UTF8,
                0,
            );
            if device.is_null() {
                return Err("出力機器の UID を作成できません。".to_string());
            }
            // 空 UID でもここでは実機器 UID を設定する。AQ の既定追従は使用しない。
            let st = AudioQueueSetProperty(
                aq,
                CURRENT_DEVICE,
                (&device as *const CFStringRef).cast(),
                std::mem::size_of::<CFStringRef>() as u32,
            );
            CFRelease(device);
            if st != 0 {
                return Err(format!("選択した出力機器に固定できません (status={st})"));
            }
            let st = AudioQueueAddPropertyListener(aq, CURRENT_DEVICE, queue_device_changed, user);
            if st != 0 {
                return Err(format!(
                    "再生キューの出力先監視に失敗しました (status={st})"
                ));
            }
            for _ in 0..4 {
                let mut buf: AudioQueueBufferRef = std::ptr::null_mut();
                let st = AudioQueueAllocateBuffer(aq, frame_bytes as u32, &mut buf);
                if st != 0 {
                    return Err(format!("再生バッファを確保できません (status={st})"));
                }
                (*buf).mAudioDataByteSize = frame_bytes as u32;
                std::ptr::write_bytes((*buf).mAudioData, 0, frame_bytes);
                let st = AudioQueueEnqueueBuffer(aq, buf, 0, std::ptr::null());
                if st != 0 {
                    return Err(format!("再生バッファを登録できません (status={st})"));
                }
            }
            queue.context.callbacks.store(true, Ordering::SeqCst);
            let st = AudioQueueStart(aq, std::ptr::null());
            if st != 0 {
                return Err(format!(
                    "選択した出力機器で再生を開始できません (status={st})"
                ));
            }
            // SetCurrentDevice/Start 内の同期通知はまだ watching=false。
            // その間の本当の経路変更も、公開前の実値検証で fail closed にする。
            queue.context.watching.store(true, Ordering::SeqCst);
            verify_queue_device(&queue)?;
            Ok(())
        })();
        if let Err(reason) = setup {
            // 部分生成した AQ も必ず所有ワーカーで破棄する。ロック保持なし。
            stop_playback(queue)?;
            return Err(reason);
        }
        eprintln!("[audio] AudioQueueStart ok ({rate}Hz, frame_bytes={frame_bytes}, UID固定)");
        Ok(queue)
    }
}

/// 音声受信サーバ(独立スレッド)。24901 で待ち受け、認証後に PCM を受け流す
pub fn start(token: String, port: u16) {
    std::thread::spawn(move || {
        use std::io::{BufRead, Read, Write};
        // 再生音量の初期値(.env で指定可。設定画面のスライダと同じ範囲)
        if let Some(v) = crate::envutil::get("KNIT_AUDIO_GAIN").and_then(|v| v.parse::<f64>().ok())
        {
            set_gain(v);
            eprintln!("[audio] KNIT_AUDIO_GAIN={} を適用しました", gain());
        }
        let bind_ip = crate::envutil::get("KNIT_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
        let listener = match std::net::TcpListener::bind((bind_ip.as_str(), port)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[audio] listen {bind_ip}:{port} 失敗: {e}(音声なしで継続)");
                return;
            }
        };
        eprintln!("[audio] listening on {bind_ip}:{port}");
        // 本線と同じ連続失敗スロットル(認証失敗の連打を鈍らせる)
        let mut throttle = knit_common::secure::FailThrottle::new();
        loop {
            let (stream, peer) = match listener.accept() {
                Ok(x) => x,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
            };
            // 本線と同じ接続元制限と暗号化(トークン不一致はハンドシェイクで弾かれる)
            if !knit_common::net::is_allowed(peer.ip()) {
                eprintln!("[audio] rejected: {peer}");
                std::thread::sleep(throttle.fail());
                continue;
            }
            eprintln!("[audio] accepted from {peer}");
            stream.set_nodelay(true).ok();
            // 送信側は無音期間も 1 秒毎にキープアライブを送るため、12 秒無音は
            // 相手の音声スレッド死亡。30 秒だと accept が直列のため再接続が
            // その分遅れる(本線の生存監視 9〜10 秒とも整合させる)
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(12)))
                .ok();
            let (r, mut w) = match knit_common::secure::accept(stream, &token, b"knit-audio") {
                Ok(x) => x,
                Err(e) => {
                    eprintln!("[audio] 暗号化ハンドシェイク失敗: {e}");
                    std::thread::sleep(throttle.fail());
                    continue;
                }
            };
            let mut reader = std::io::BufReader::new(r);
            // 形式の申告: "SDAUDIO3 <rate> s16\n" → "ok\n"(認証は暗号化で済んでいる)
            let mut line = String::new();
            if (&mut reader).take(512).read_line(&mut line).unwrap_or(0) == 0 {
                std::thread::sleep(throttle.fail());
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            let s16 = parts.len() == 3 && parts[0] == "SDAUDIO3" && parts[2] == "s16";
            if !s16 {
                eprintln!("[audio] invalid handshake");
                let _ = w.write_all(b"ng\n");
                std::thread::sleep(throttle.fail());
                continue;
            }
            let Ok(rate) = parts[1].parse::<u32>() else {
                std::thread::sleep(throttle.fail());
                continue;
            };
            if !(4000..=192_000).contains(&rate) {
                std::thread::sleep(throttle.fail());
                continue;
            }
            if w.write_all(b"ok\n").and_then(|_| w.flush()).is_err() {
                std::thread::sleep(throttle.fail());
                continue;
            }
            throttle.success();
            // ネットワーク経路も低遅延優先(全経路で ≈42ms のプリロール)
            PRE_ROLL.store(16 * 1024, Ordering::Relaxed);
            ensure_playback(rate);
            RING.lock().unwrap_or_else(|e| e.into_inner()).clear();
            PRIMED.store(false, Ordering::Relaxed);
            phase_store(0.0);
            eprintln!("[audio] streaming started ({rate}Hz s16/stereo, 暗号化)");
            // PCM フレーム受信: [u32 LE 長][データ]。長さ上限は 128KB
            let mut len_buf = [0u8; 4];
            let mut last_diag = std::time::Instant::now();
            loop {
                if reader.read_exact(&mut len_buf).is_err() {
                    break;
                }
                let len = u32::from_le_bytes(len_buf) as usize;
                if len == 0 {
                    // キープアライブ(無音期間の死活監視用)。
                    // 無音期間はここがループの唯一の出口のため diag も出す
                    diag_log(&mut last_diag, rate);
                    continue;
                }
                if len > 128 * 1024 {
                    eprintln!("[audio] invalid frame len={len}. disconnect");
                    break;
                }
                let mut frame = vec![0u8; len];
                if reader.read_exact(&mut frame).is_err() {
                    break;
                }
                if s16 {
                    // 再生リングは f32 のまま(補間・追い込み処理を共通にする)。
                    // 受信スレッドでゲインを掛ける(設定スライダの音量反映)。
                    // 振り切れは clamp で防ぐ(第2レーンの加算と同じ保護)
                    let g = gain();
                    frame = frame
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .flat_map(|b| {
                            let v = ((i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0) * g)
                                .clamp(-1.0, 1.0);
                            v.to_le_bytes()
                        })
                        .collect();
                }
                // フレーム整合の防御: 8 バイト(f32×2ch)境界に切り詰める。
                // 送信側はフレーム単位で送るはずだが、万一の途中欠けが
                // 混入しても破壊音として再生されないようにする
                frame.truncate(frame.len() & !7usize);
                if frame.is_empty() {
                    continue;
                }
                RX_BYTES.fetch_add(len as u64, Ordering::Relaxed);
                if !MUTED.load(Ordering::Relaxed) && output_permitted() {
                    let mut ring = RING.lock().unwrap_or_else(|e| e.into_inner());
                    // ミュート明けに古い音を鳴らさない、かつ遅延を溜めない:
                    // 上限超過分は古い方から捨てる(8バイト境界で)
                    if ring.len() + frame.len() > RING_CAP {
                        let overflow = (ring.len() + frame.len() - RING_CAP) & !7usize;
                        let n = overflow.min(ring.len());
                        ring.drain(..n);
                    }
                    ring.extend(frame.iter().copied());
                } else {
                    // ミュート明けはプリロールからやり直す(クッション無しの
                    // 即再生で断続音が出るのを防ぐ)
                    PRIMED.store(false, Ordering::Relaxed);
                    phase_store(0.0);
                }
                // 鳴っている間の diag(キープアライブ時と同じ関数・同じ形式で
                // drop も出す。10 秒未満の呼び出しは内部で弾かれる)
                diag_log(&mut last_diag, rate);
            }
            eprintln!("[audio] stream ended");
        }
    });
}

/// 同一プロセス内(Android タブレットの中継)からの音声供給。ネットワーク経路と
/// 同じ再生リングへ流す。ポート待受(24901)は accept が直列のため、外部からの
/// 接続失敗が続くと中継の接続が accept までタイムアウトする。これを避ける専用口
pub fn feed_s16(rate: u32, pcm_s16: &[u8]) {
    ensure_playback(rate);
    // Android 中継は USB 経由の安定供給のため低遅延優先(プリロールは全経路共通)。
    // 音量: 実機では小さめに届くため持ち上げる。Windows 経路と加算ミックスするため
    // ×1.5 に抑える(合計の振り切れは加算後のソフト飽和で保護)
    const GAIN: f32 = 1.5;
    let mut frame: Vec<u8> = pcm_s16
        .as_chunks::<2>()
        .0
        .iter()
        .flat_map(|b| {
            let v = ((i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0) * GAIN).clamp(-1.0, 1.0);
            v.to_le_bytes()
        })
        .collect();
    frame.truncate(frame.len() & !7usize);
    if frame.is_empty() {
        return;
    }
    RX_BYTES.fetch_add(pcm_s16.len() as u64, Ordering::Relaxed);
    if !MUTED.load(Ordering::Relaxed) && output_permitted() {
        let mut ring = RING_B.lock().unwrap_or_else(|e| e.into_inner());
        // 滞留の上限は約 80ms。超過分は一度に全部ではなく 1/8 ずつ捨てる
        // (一括で捨てると波形が周期的に途切れて「プツプツ/割れ」に聞こえる)
        // 48kHz f32 stereo で 80ms ≒ 30KB(旧値は実質 800ms で、映像とのずれを溜めていた)
        const ANDROID_MAX_BACKLOG: usize = 48_000 * 2 * 4 * 80 / 1000;
        while ring.len() > ANDROID_MAX_BACKLOG {
            let excess = ((ring.len() - ANDROID_MAX_BACKLOG) / 8) & !7usize;
            let n = excess.max(8).min(ring.len());
            DROP_BYTES.fetch_add(n as u64, Ordering::Relaxed);
            ring.drain(..n);
        }
        ring.extend(frame.iter().copied());
    } else {
        PRIMED_B.store(false, Ordering::Relaxed);
    }
}

/// 第2レーン(Android 中継)の在庫を捨てる(セッション終了時に呼ぶ)
pub fn clear_lane_b() {
    RING_B.lock().unwrap_or_else(|e| e.into_inner()).clear();
    PRIMED_B.store(false, Ordering::Relaxed);
}

/// 60 秒毎の診断ログ(受信量/再生量/滞留 lag/間引き drop)。
/// 無音期間はキープアライブ受信時に、鳴っている間はフレーム受信時に呼ばれる。
/// drop が増え続けていれば波形を切っている=音割れの原因として疑う。
/// 10 秒→60 秒へ緩和: 常時接続で log が約 1MB/日で育ち続けるのを抑えるため
fn diag_log(last_diag: &mut std::time::Instant, rate: u32) {
    if last_diag.elapsed() < std::time::Duration::from_secs(60) {
        return;
    }
    *last_diag = std::time::Instant::now();
    let rx = RX_BYTES.load(Ordering::Relaxed) / 1024;
    let play = PLAY_BYTES.load(Ordering::Relaxed) / 1024;
    let queued = RING.lock().unwrap_or_else(|e| e.into_inner()).len();
    let lag_ms = queued as u64 * 1000 / (rate as u64 * 8);
    let drop = DROP_BYTES.swap(0, Ordering::Relaxed) / 1024;
    eprintln!(
        "[audio] diag rx={rx}KB played={play}KB queued={}KB lag={lag_ms}ms drop={drop}KB",
        queued / 1024
    );
}

#[cfg(test)]
mod output_tests {
    use super::*;

    // --emit=metadata のみでも ABI の取り違えを検出できる。
    const _: () = {
        assert!(std::mem::size_of::<AudioObjectPropertyAddress>() == 12);
        assert!(std::mem::size_of::<AudioStreamBasicDescription>() == 40);
        assert!(std::mem::size_of::<AudioQueueBuffer>() == 56);
        assert!(std::mem::offset_of!(AudioQueueBuffer, mAudioData) == 8);
        assert!(std::mem::offset_of!(AudioQueueBuffer, mAudioDataByteSize) == 16);
        assert!(std::mem::offset_of!(AudioQueueBuffer, mPacketDescriptionCount) == 48);
    };

    fn target(id: u32, uid: &str) -> OutputTarget {
        OutputTarget {
            id,
            uid: uid.to_string(),
            streams: vec![id + 10],
            sources: Some(vec![1]),
            jack: Some(vec![1]),
        }
    }

    fn selection(uid: &str) -> OutputSelection {
        OutputSelection {
            uid: uid.to_string(),
            target: None,
            generation: 1,
            status: None,
        }
    }

    #[test]
    fn default_is_pinned_and_never_silently_follows_another_device() {
        let mut selected = selection("");
        let headphones = target(2, "headphones");
        assert!(!selected.observe(1, Ok(headphones.clone())));
        assert!(!selected.observe(1, Ok(headphones.clone())));
        assert!(selected.observe(1, Ok(target(3, "speakers"))));
        assert_eq!(selected.uid, ""); // 保存値は「既定」のまま
        assert_eq!(selected.target, Some(headphones.clone()));
        assert!(selected.status.is_some());
        assert!(!selected.observe(1, Ok(headphones))); // 戻っても自動復帰しない
        assert!(selected.status.is_some());
    }

    #[test]
    fn missing_saved_uid_is_retained_and_cannot_bind_system_default() {
        let mut selected = selection("");
        let old = selected.generation;
        assert!(selected
            .restore("missing-usb", Err("missing".into()))
            .is_err());
        assert_eq!(selected.uid, "missing-usb");
        assert!(selected.target.is_none());
        assert_eq!(selected.status.as_deref(), Some("missing"));
        assert!(selected.generation > old);
        assert!(!selected.observe(selected.generation, Ok(target(2, "speakers"))));
        assert!(selected.target.is_none());
        selected.select("missing-usb", target(5, "missing-usb"));
        assert!(selected.status.is_none());
        assert_eq!(selected.target.as_ref().unwrap().uid, "missing-usb");
    }

    #[test]
    fn same_uid_physical_route_and_stream_changes_invalidate() {
        let original = target(2, "built-in");
        for field in 0..4 {
            let mut selected = selection("built-in");
            selected.select("built-in", original.clone());
            let mut changed = original.clone();
            match field {
                0 => changed.id = 99,
                1 => changed.streams = vec![99],
                2 => changed.sources = Some(vec![2]),
                _ => changed.jack = Some(vec![0]),
            }
            assert!(selected.observe(selected.generation, Ok(changed)));
            assert!(selected.status.is_some());
            assert_eq!(selected.target, Some(original.clone()));
        }
    }

    #[test]
    fn missing_device_latches_until_explicit_reselection() {
        let mut selected = selection("usb");
        selected.select("usb", target(4, "usb"));
        let previous = selected.generation;
        assert!(selected.observe(previous, Err("disconnected".to_string())));
        assert!(!selected.observe(previous, Ok(target(4, "usb"))));
        assert_eq!(selected.status.as_deref(), Some("disconnected"));
        selected.select("usb", target(5, "usb"));
        assert_ne!(selected.generation, previous);
        assert!(selected.status.is_none());
        // 旧 HAL 問い合わせの遅い完了が新選択を失効させない。
        assert!(!selected.observe(previous, Err("old result".to_string())));
        assert!(selected.status.is_none());
        assert!(!selected.observe(selected.generation, Ok(target(5, "usb"))));
    }

    #[test]
    fn generation_is_monotonic_and_errors_do_not_bind_default() {
        let mut selected = selection("");
        assert!(selected.observe(1, Err("no output".to_string())));
        assert!(selected.target.is_none());
        selected.generation = u64::MAX;
        selected.select("", target(2, "default"));
        assert_eq!(selected.generation, u64::MAX);
        assert!(selected.status.is_none());
    }

    fn queue_context(generation: u64, identity: usize) -> (QueueContext, AudioQueueRef) {
        let context = QueueContext::new(generation);
        let aq = std::ptr::without_provenance_mut(identity);
        context.queue.store(aq, Ordering::SeqCst);
        (context, aq)
    }

    #[test]
    fn stale_queue_notification_stays_with_its_owner_after_reselection() {
        let (old, aq) = queue_context(7, 1);
        // AQ アドレスが再利用されても、user/context は異なる所有者に固定される。
        let (new, _) = queue_context(8, 1);
        old.watching.store(true, Ordering::SeqCst);
        new.watching.store(true, Ordering::SeqCst);
        assert!(old.notify_device_change(aq, CURRENT_DEVICE));
        assert_eq!(old.generation, 7);
        assert!(!old.device_verified());
        assert_eq!(new.generation, 8);
        assert!(new.device_verified());
        assert_eq!(new.device_changes.load(Ordering::SeqCst), 0);
        old.disable();
        assert!(!old.notify_device_change(aq, CURRENT_DEVICE));
        assert!(new.watching.load(Ordering::SeqCst));
    }

    #[test]
    fn queue_notification_checks_identity_property_and_stopping_gate() {
        let (context, aq) = queue_context(7, 1);
        context.watching.store(true, Ordering::SeqCst);
        assert!(!context.notify_device_change(std::ptr::null_mut(), CURRENT_DEVICE));
        assert!(!context.notify_device_change(std::ptr::without_provenance_mut(2), CURRENT_DEVICE));
        assert!(!context.notify_device_change(aq, DEFAULT_OUTPUT));
        assert!(context.device_verified());
        context.callbacks.store(true, Ordering::SeqCst);
        context.disable();
        assert!(!context.callbacks.load(Ordering::SeqCst));
        assert!(!context.notify_device_change(aq, CURRENT_DEVICE));
    }

    #[test]
    fn startup_and_delayed_self_notifications_do_not_latch_valid_queue() {
        let (context, aq) = queue_context(7, 1);
        // 同期 SetCurrentDevice/Start 通知は arm 前に発生する。
        for _ in 0..2 {
            assert!(!context.notify_device_change(aq, CURRENT_DEVICE));
        }
        context.watching.store(true, Ordering::SeqCst);
        assert!(context.confirm_device(0, "usb", Ok("usb".into())).is_ok());
        assert!(context.device_verified());
        // 遅延した自己通知も実 UID が同じなら再確認後に復帰できる。
        assert!(context.notify_device_change(aq, CURRENT_DEVICE));
        assert!(!context.device_verified());
        let observed = context.device_changes.load(Ordering::SeqCst);
        assert!(context
            .confirm_device(observed, "usb", Ok("usb".into()))
            .is_ok());
        assert!(context.device_verified());
    }

    #[test]
    fn queue_verification_cannot_acknowledge_a_notification_during_the_read() {
        let (context, aq) = queue_context(7, 1);
        context.watching.store(true, Ordering::SeqCst);
        context.notify_device_change(aq, CURRENT_DEVICE);
        let observed = context.device_changes.load(Ordering::SeqCst);
        context.notify_device_change(aq, CURRENT_DEVICE);
        context
            .confirm_device(observed, "usb", Ok("usb".into()))
            .unwrap();
        assert!(!context.device_verified());
        let observed = context.device_changes.load(Ordering::SeqCst);
        context
            .confirm_device(observed, "usb", Ok("usb".into()))
            .unwrap();
        assert!(context.device_verified());
    }

    #[test]
    fn queue_route_mismatch_or_query_error_remains_closed() {
        let (context, aq) = queue_context(7, 1);
        context.watching.store(true, Ordering::SeqCst);
        context.notify_device_change(aq, CURRENT_DEVICE);
        let observed = context.device_changes.load(Ordering::SeqCst);
        assert!(context
            .confirm_device(observed, "usb", Ok("speakers".into()))
            .is_err());
        assert!(!context.device_verified());
        assert!(context
            .confirm_device(observed, "usb", Err("query failed".into()))
            .is_err());
        assert!(!context.device_verified());
    }

    #[test]
    fn topology_notification_token_never_borrows_the_current_generation() {
        assert_eq!(topology_generation(std::ptr::null_mut()), None);
        for generation in [1, 7, u64::MAX] {
            assert_eq!(
                topology_generation(topology_user(generation)),
                Some(generation)
            );
        }
        let mut selected = selection("usb");
        selected.select("usb", target(4, "usb"));
        let old_user = topology_user(selected.generation);
        selected.select("usb", target(5, "usb"));
        let old_generation = topology_generation(old_user).unwrap();
        assert_ne!(old_generation, selected.generation);
        assert!(!selected.observe(old_generation, Err("old HAL notification".into())));
        assert!(selected.status.is_none());
    }

    #[test]
    fn core_audio_abi_layout_matches_64_bit_sdk() {
        assert_eq!(std::mem::size_of::<AudioObjectPropertyAddress>(), 12);
        assert_eq!(std::mem::size_of::<AudioStreamBasicDescription>(), 40);
        assert_eq!(std::mem::size_of::<AudioQueueBuffer>(), 56);
        assert_eq!(std::mem::offset_of!(AudioQueueBuffer, mAudioData), 8);
        assert_eq!(
            std::mem::offset_of!(AudioQueueBuffer, mAudioDataByteSize),
            16
        );
        assert_eq!(
            std::mem::offset_of!(AudioQueueBuffer, mPacketDescriptionCount),
            48
        );
        assert_eq!(CURRENT_DEVICE, 0x6171_6364);
        assert_eq!(DEFAULT_OUTPUT, 0x644f_7574);
    }
}
