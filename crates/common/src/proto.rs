    use serde::{Deserialize, Serialize};

    pub const PORT: u16 = 24900;
    /// プロトコル版。9: Leave 追加・Focus/Minimize 削除・版交渉(MIN_VERSION)導入。
    /// 10: ファイル・画像を大容量経路(bulk, 24902)へ移し本線から File*/ClipData を削除。
    /// 11: 全経路を Noise 暗号化(認証はハンドシェイクで行い hello のトークンは空)
    /// 12: Windows→Macの操作ID付きファイルドラッグ。
    /// 13: hello/hello_ok に端末 id と全モニター構成(monitors)を追加(複数台接続の土台)。
    /// 14: ファイル転送の拡張。フォルダ(相対パス)対応・FILE_END での内容ハッシュ
    ///     (BLAKE2s)検証・mtime 保持・DragOffer/DragCancel の容量拡大(512 件)。
    ///     旧版相手はフォルダを送れないため、版 14 未満へのフォルダ送信は呼び出し側で止める
    /// 15: 転送結果の相互確認。FILE_BEGIN の "dir" 印(空フォルダの再現)・
    ///     受理結果を返す XferAck・役割切替の RoleAck・Cfg の files/listen ビット。
    ///     いずれも旧側は未知の行・未知のフィールドとして無視するため MIN_VERSION は据え置き
    pub const VERSION: u32 = 15;
    /// 接続を受け入れる最小の相手版。新しいメッセージは未知として無視される
    /// (decode が None を返す)ため、MIN_VERSION 以上なら新旧混在でも通信できる。
    /// 片側だけ更新された状態で接続拒否が続く事故を防ぐ
    pub const MIN_VERSION: u32 = 11;

    /// 相手の版を受け入れてよいか
    pub fn compatible(peer: u32) -> bool {
        peer >= MIN_VERSION
    }

    /// 相手の版が持つファイル転送の機能一式。版ごとの差分はこの判定に集約し、
    /// 呼び出し側が閾値リテラル(ver >= 14 等)を個別に書かないようにする
    ///(片側だけ更新される静かな不整合を防ぐ)
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PeerFeatures {
        /// フォルダ(相対パス)を送れる(版 14 以降)
        pub dirs: bool,
        /// 空フォルダの再現("dir" 印)ができる(版 15 以降)
        pub empty_dirs: bool,
    }

    pub fn peer_features(ver: u32) -> PeerFeatures {
        PeerFeatures {
            dirs: ver >= 14,
            empty_dirs: ver >= 15,
        }
    }

    /// 1 枚のモニター(仮想画面座標系での位置とサイズ。Mac は CG 座標系のまま)。
    /// 版 13 以降の hello/hello_ok で交換し、複数モニターの自動認知に使う。
    /// 旧版からの受信は空配列(既定)になり、従来どおり w/h の 1 画面扱い
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    pub struct Monitor {
        #[serde(default)]
        pub x: i32,
        #[serde(default)]
        pub y: i32,
        pub w: i32,
        pub h: i32,
        /// 機種名などの表示名。旧版との相互接続のため省略可(空=不明)
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub name: String,
    }

    impl Monitor {
        /// モニター群の合計面積を本文として表す(ログ・表示用の 1 行)
        pub fn summary(monitors: &[Monitor]) -> String {
            if monitors.is_empty() {
                return "不明".to_string();
            }
            monitors
                .iter()
                .map(|m| format!("{}x{}@{},{}", m.w, m.h, m.x, m.y))
                .collect::<Vec<_>>()
                .join(" + ")
        }
    }

    /// hello/hello_ok の monitors として受け付ける件数上限。
    /// 実用上 8 面で十分(異常に長い配列でレイアウト計算を引っ張らせない)
    pub const MAX_MONITORS: usize = 8;

    /// 1 枚のモニターの値域。w/h は Screen と同じ 1..=32768、x/y は仮想画面
    /// 座標として両方向へ同じ幅を許す(-32768..=32768)。
    /// hello は暗号化ハンドシェイクの先だが相手の値をそのままレイアウトへ
    /// 使うため、Screen の受信ガードと対称に検査する
    pub fn monitor_valid(m: &Monitor) -> bool {
        (1..=32768).contains(&m.w)
            && (1..=32768).contains(&m.h)
            && (-32768..=32768).contains(&m.x)
            && (-32768..=32768).contains(&m.y)
    }

    /// 受信したモニター群から値域外の物を除き、件数を上限へ切り詰める。
    /// 全て不正なら空配列(= 従来どおり w/h の単一画面扱い)。
    /// 除外は「接続を切らない」方向の無害化: 旧版相互接続で空配列が来る
    /// 経路と同じ状態に落ち着く
    pub fn sanitize_monitors(monitors: &[Monitor]) -> Vec<Monitor> {
        monitors
            .iter()
            .filter(|m| monitor_valid(m))
            .take(MAX_MONITORS)
            .cloned()
            .collect()
    }

    /// 端末識別子(初回に生成して保存)。暗号用途ではなく、同じ Mac へ
    /// 接続した複数端末と、同一端末の再接続を区別するための値。
    /// 再起動で変わってはならない: 接続先の配置・選択の保存が id に紐づくため、
    /// 起動ごとに作り直すと毎回「別の端末」扱いになる(実機で発生)
    pub fn device_id() -> String {
        static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        ID.get_or_init(|| {
            let path = crate::envutil::data_dir().map(|d| d.join("device-id.txt"));
            if let Some(p) = &path {
                if let Some(id) = load_device_id_from(p) {
                    return id;
                }
            }
            let mut seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0)
                ^ ((std::process::id() as u64) << 32);
            let mut next = move || {
                // splitmix64: 識別子生成には十分な拡散
                seed = seed.wrapping_add(0x9e3779b97f4a7c15);
                let mut z = seed;
                z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
                z ^ (z >> 31)
            };
            let id = format!("{:016x}", next());
            if let Some(p) = &path {
                if let Err(e) = save_device_id_to(p, &id) {
                    // 毎回新しい端末として振る舞うことになる(選択端末の維持が効かない)ため記録する
                    eprintln!("[proto] 端末 id を保存できません: {e}");
                }
            }
            id
        })
        .clone()
    }

    /// device-id.txt から有効な id を読む。検査に落ちる(壊れている)場合は
    /// 上書きで失われる前に .corrupt へ退避して None を返す。無言で新しい id を
    /// 生成すると peer-sides 等の id 紐付けが全て切れるため
    fn load_device_id_from(path: &std::path::Path) -> Option<String> {
        let Ok(s) = std::fs::read_to_string(path) else {
            return None;
        };
        let s = s.trim();
        if !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Some(s.to_string());
        }
        let backup = crate::persist::quarantine(path);
        eprintln!(
            "[proto] device-id.txt が壊れています。{}新しい id を生成します(旧 id での紐付けは失われます)",
            backup
                .as_ref()
                .map(|p| format!("{} へ退避しました。", p.display()))
                .unwrap_or_default()
        );
        None
    }

    /// 生成した id の保存(一時ファイル+sync+rename のアトミック置換)
    fn save_device_id_to(path: &std::path::Path, id: &str) -> std::io::Result<()> {
        crate::persist::write_atomic(path, id.as_bytes())
    }

    /// 通信で受け取った表示名を UI・ログへ載せられる形にする: 制御文字と
    /// 文字の並びを偽装する Bidi オーバーライドを除去し、長さを切る。
    /// 登録(pairing)の safe_name と同一規則を hello の name にも適用する
    pub fn safe_peer_name(name: &str) -> String {
        name.chars()
            .filter(|c| {
                // 制御文字と、見た目を偽装できる書式文字(Bidi・ゼロ幅・方向マーク)を除く
                !c.is_control()
                    && !matches!(
                        *c,
                        '\u{202a}'..='\u{202e}'
                            | '\u{2066}'..='\u{2069}'
                            | '\u{200b}'..='\u{200f}'
                            | '\u{2060}'..='\u{2064}'
                            | '\u{061c}'
                            | '\u{feff}'
                    )
            })
            .take(48)
            .collect()
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum TabletAction {
        Back,
        PreviousApp,
        NextApp,
        Home,
        Recents,
        Screenshot,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(tag = "t")]
    pub enum Msg {
        #[serde(rename = "hello")]
        Hello {
            ver: u32,
            name: String,
            /// 版 11 以降は空(認証は暗号化ハンドシェイクで済んでいる)
            #[serde(default)]
            token: String,
            /// 送信側の画面幅/高さ(px)。スケール自動算出と絶対座標送信に使う
            #[serde(default)]
            w: i32,
            #[serde(default)]
            h: i32,
            /// 端末識別子(版 13 以降)。複数台の区別と再接続の紐付けに使う
            #[serde(default)]
            id: String,
            /// 送信側の全モニター(版 13 以降)。旧版からの受信は空
            #[serde(default)]
            monitors: Vec<Monitor>,
        },
        #[serde(rename = "hello_ok")]
        HelloOk {
            name: String,
            w: i32,
            h: i32,
            #[serde(default)]
            ver: u32,
            /// 端末識別子(版 13 以降)
            #[serde(default)]
            id: String,
            /// 受け側の全モニター(版 13 以降)
            #[serde(default)]
            monitors: Vec<Monitor>,
        },
        /// 越境ドラッグの受け渡しID。Mac 側の生成値は最上位ビット(1<<63)を立て、
        /// DragCancel を「相手発のドラッグ中止」ではなく「自分送信中の転送中止要求」
        /// として解釈する合図に使う。Windows 側はミリ秒由来の小さい値を採番する
        #[serde(rename = "drag_offer")]
        DragOffer {
            id: u64,
            count: usize,
            total: u64,
            position: f64,
        },
        #[serde(rename = "drag_accept")]
        DragAccept { id: u64 },
        #[serde(rename = "drag_ready")]
        DragReady { id: u64 },
        #[serde(rename = "drag_commit")]
        DragCommit { id: u64 },
        /// id の最上位ビットが 1 のとき、送信側(Mac)はこれを「自分が送信中の
        /// 転送の中止要求」として解釈する(相手の自発中止とは区別する)
        #[serde(rename = "drag_cancel")]
        DragCancel { id: u64 },
        #[serde(rename = "drag_done")]
        DragDone { id: u64, copied: bool },
        /// 画面構成の変化(Windows→Mac)。解像度変更・モニター抜き差しで送る
        #[serde(rename = "screen")]
        Screen { w: i32, h: i32 },
        /// ゲームモード(Windows→Mac)。カーソルが閉じ込められた/全画面で隠れた間は
        /// 相対移動で送ってほしい(絶対座標では視点回転が効かない)
        #[serde(rename = "rel")]
        Rel { on: bool },
        /// 画面ロックの連動(Mac→Windows)。Mac がロックされたら Windows もロックする
        #[serde(rename = "lock")]
        Lock,
        #[serde(rename = "key")]
        Key {
            kc: u16,
            down: bool,
            ctrl: bool,
            opt: bool,
            cmd: bool,
            shift: bool,
            /// 翻訳済みキー(⌘]→Tab 等)。Win 側の ⌘Tab→Alt+Tab 変換など
            /// 「生の Mac 入力」前提の特殊処理を適用しない
            #[serde(default)]
            tr: bool,
            /// 右⌘を右 Ctrl として扱う(Mac 側で cmd から分離して載せる)。
            /// 旧側は未知フィールドを無視するため版 11 のまま
            #[serde(default)]
            rcmd: bool,
        },
        #[serde(rename = "mouse_move")]
        MouseMove { dx: f64, dy: f64 },
        /// カーソル絶対位置(0..1 正規化)。Windows 側は MOUSEEVENTF_ABSOLUTE で注入し、
        /// ポインタ加速曲線を通さず Mac の速度感をそのまま再現する
        #[serde(rename = "mouse_abs")]
        MouseAbs { nx: f64, ny: f64 },
        #[serde(rename = "mouse_btn")]
        MouseButton { btn: u8, down: bool },
        #[serde(rename = "scroll")]
        Scroll { dx: f64, dy: f64 },
        /// MacのピンチをAndroidの2本指タッチへ変換する。phase: 0開始/1更新/2終了/3取消。
        #[serde(rename = "pinch")]
        Pinch { delta: f64, phase: u8 },
        /// タブレットの設定ページに合わせたナビゲーション操作。
        #[serde(rename = "tablet_gesture")]
        TabletGesture { action: TabletAction },
        /// 複数端末の選択状態。Android専用アプリは選択中のみ大容量経路を保持する。
        #[serde(rename = "selected")]
        Selected { on: bool },
        /// 専用Androidアプリからの表示寸法・許可状態。旧版は無視する。
        #[serde(rename = "tablet_info")]
        TabletInfo { width_mm: f64, height_mm: f64, control: bool, keyboard: bool, #[serde(default)] japanese: bool },
        /// OSのIMEで確定したUnicode文字列を専用Android IMEへ渡す。
        #[serde(rename = "text")]
        Text { text: String },
        /// Windows 左端到達による復帰通知。ny = 復帰時のカーソル高さ(0..1、Mac 側復帰位置へ反映)
        #[serde(rename = "return")]
        Return {
            #[serde(default)]
            ny: f64,
        },
        /// クリップボード同期(プレーンテキスト)
        #[serde(rename = "clip")]
        Clip { text: String },
        /// Mac が制御を取り戻した(Windows を離れた)。Windows は押下中の全キー・
        /// ボタン・Alt+Tab を解放する。ホットキー/Mac 内完結の左端復帰/切断など
        /// Windows が自力で検知できない離脱経路のための後片付け合図
        #[serde(rename = "leave")]
        Leave,
        /// カーソル絶対ワープ(0..1 正規化。切替時に相手画面の対応位置へ飛ばす)
        #[serde(rename = "warp")]
        Warp { nx: f64, ny: f64 },
        #[serde(rename = "ping")]
        Ping {
            /// 送信時刻(unix ms)。Pong にエコーバックされ RTT 測定に使う
            #[serde(default)]
            ts: u64,
        },
        #[serde(rename = "pong")]
        Pong {
            #[serde(default)]
            ts: u64,
        },
        /// 設定同期: ⌘キーのマップ先(false=Ctrl 既定 / true=Alt)と
        /// Windows スピーカーのミュート(true=接続中ミュート=Mac のみ発音)。
        /// 接続確立時とメニュー切替時に Mac→Windows へ送る
        #[serde(rename = "cfg")]
        Cfg {
            cmd_alt: bool,
            #[serde(default)]
            spk_mute: bool,
            /// Windows 画面の位置(0=Macの右/1=左/2=上/3=下。Deskflow の links 相当)
            #[serde(default)]
            side: u8,
            /// クリップボード共有。false の間は Windows 側も送らない(相手任せにしない)
            #[serde(default = "default_true")]
            clip: bool,
            /// ファイル共有(版 15 以降)。false の間は Windows 側もファイルを送らない。
            /// clip と同じく「相手が受け取らないものは送らない」ための双方向の合図
            #[serde(default = "default_true")]
            files: bool,
            /// この Mac が相手の音声を再生する意思(版 15 以降)。false の間、
            /// 相手は音声ストリームの送信を止めてよい(誰も聞いていない間の帯域を省く)
            #[serde(default = "default_true")]
            listen: bool,
        },
        /// Windows の音量制御(0=up / 1=down / 2=ミュート)。Mac メニューから送る
        #[serde(rename = "vol")]
        Vol { op: u8 },
        /// 接続品質通知: Mac が測定した RTT(ms)を Windows 側の表示へ回す
        #[serde(rename = "stat")]
        Stat { rtt: u64 },
        /// IME 状態同期(Mac→Windows、画面を移る時に送る)。Mac のかな/英数を
        /// Windows 側 IME の開閉へ反映する(ビジョン§7 IME Follow Cursor)
        #[serde(rename = "ime")]
        Ime { kana: bool },
        /// Caps Lock 状態同期(Mac→Windows、画面を移る時に送る)。Mac の
        /// alphaShift と Windows の VK_CAPITAL トグルがズレていると越境直後の
        /// 大文字/小文字が反転するため、入りの時点で合わせる。旧側は未知行と
        /// して無視する拡張(版 15 のまま)
        #[serde(rename = "caps")]
        Caps { on: bool },
        /// Continue Here(ビジョン§11): 相手側の既定ブラウザで開く URL。
        /// スキーム・長さの検査は urlx::transferable で両側で行う
        #[serde(rename = "open_url")]
        OpenUrl { url: String },
        /// 越境 App Handoff(ビジョン§12): 相手 PC のアプリ一覧を要求する。
        /// 旧側は未知行として無視する拡張(版 12 のまま)
        #[serde(rename = "apps_query")]
        AppsQuery,
        /// アプリ一覧の応答((表示名, 起動パス)。起動は RunApp で、受け側が
        /// この列挙結果と突き合わせてから行う)
        #[serde(rename = "apps_reply")]
        AppsReply { apps: Vec<(String, String)> },
        /// 相手 PC でアプリを起動する。受け側は直前に列挙したパスと
        /// 完全一致するものだけ実行する(任意パスの実行を防ぐ)
        #[serde(rename = "run_app")]
        RunApp { path: String },
        /// 接続の方向の切替の通知(どちらからでも)。host=true は「送り手が次の起動から
        /// ホスト(待ち受け側)になる」。受け手は反対の役割(接続側)へ合わせて再起動する。
        /// 旧側は未知行として無視する拡張(版 14 のまま)
        #[serde(rename = "role")]
        Role { host: bool },
        /// 役割切替の適用済み通知(版 15 以降)。Role を受け取った側は保存を終えて
        /// 再起動へ移る前に必ず返す。送り手はこれを確認してから再起動する
        #[serde(rename = "role_ack")]
        RoleAck,
        /// 大容量転送の受理結果(版 15 以降)。受信側がバッチ完了時に本線で返す。
        /// accepted=保存できた件数、rejected=保存できなかった件数、
        /// scope=true は受信側の共有設定で受け取りを拒否した分があったことを示す。
        /// 旧側は未知行として無視するため、旧版相手は応答が返らず呼び出し側の
        /// タイムアウトで「確認できず」扱いになる
        #[serde(rename = "xfer_ack")]
        XferAck {
            accepted: usize,
            rejected: usize,
            #[serde(default)]
            scope: bool,
        },
        /// 設定の遠隔操作(Windows→Mac): Mac の設定一覧を要求する。
        /// 旧側は未知行として無視する拡張(版 14 のまま)
        #[serde(rename = "prefs_get")]
        PrefsGet,
        /// Mac の設定一覧(Mac→Windows)。JSON 文字列(キーは Mac の preferences.json と同じ)
        #[serde(rename = "prefs")]
        Prefs { json: String },
        /// Mac の設定を変える(Windows→Mac)。変えたいキーだけを持つ JSON 文字列。
        /// 受け側が許可したキーと範囲だけを適用し、結果の一覧を Prefs で返す
        #[serde(rename = "prefs_set")]
        PrefsSet { json: String },
        #[serde(rename = "bye")]
        Bye,
    }

    fn default_true() -> bool {
        true
    }

    pub fn encode(msg: &Msg) -> String {
        let mut s = serde_json::to_string(msg).unwrap_or_default();
        s.push('\n');
        s
    }

    /// Vol の op のうちメディア制御(3=前へ/4=再生・一時停止/5=次へ)に対応する
    /// Windows のメディア VK。op 0-2(音量)は None。両側で意味の対応を
    /// 1 箇所で保証するためにここへ置く
    pub fn media_vk(op: u8) -> Option<u16> {
        Some(match op {
            3 => 0xB1, // VK_MEDIA_PREV_TRACK
            4 => 0xB3, // VK_MEDIA_PLAY_PAUSE
            5 => 0xB0, // VK_MEDIA_NEXT_TRACK
            _ => return None,
        })
    }

    pub fn decode(line: &str) -> Option<Msg> {
        serde_json::from_str(line.trim()).ok()
    }

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("knit-devid-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 有効な id はそのまま読まれ、保存はアトミックに書かれる
    #[test]
    fn device_id_round_trips_through_disk() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("device-id.txt");
        save_device_id_to(&path, "0123456789abcdef").unwrap();
        // 前後の空白は読み込み側で落とす(エディタで開いた痕跡への耐性)
        std::fs::write(&path, " 0123456789abcdef\n").unwrap();
        assert_eq!(load_device_id_from(&path).as_deref(), Some("0123456789abcdef"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 壊れた id ファイル: 無言で新 id を作らず、退避してから None を返す。
    /// 退避先には元のバイトが残る(何が起きたか後から調べられる)
    #[test]
    fn broken_device_id_is_quarantined_and_regenerated() {
        let dir = temp_dir("broken");
        let path = dir.join("device-id.txt");
        for bad in ["", "  \n", "not!alphanumeric", &"x".repeat(65)] {
            std::fs::write(&path, bad).unwrap();
            assert!(load_device_id_from(&path).is_none(), "壊れた id は拒否: {bad:?}");
            assert!(!path.exists(), "壊れたファイルは元の位置に残さない");
            let backup = dir.join("device-id.txt.corrupt");
            assert_eq!(
                std::fs::read_to_string(&backup).unwrap(),
                bad,
                "退避先に元の内容が残る"
            );
            // 次の生成: 退避を壊さず新しい id が書ける
            save_device_id_to(&path, "abc123").unwrap();
            assert_eq!(load_device_id_from(&path).as_deref(), Some("abc123"));
            // 次の壊れケースに備えて掃除(退避ファイルの番号増加を確認するのは
            // persist::quarantine の単体テスト側で行う)
            let _ = std::fs::remove_file(dir.join("device-id.txt.corrupt"));
            let _ = std::fs::remove_file(dir.join("device-id.txt.corrupt.1"));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 存在しないファイルは退避を作らない(初回起動と破損を区別する)
    #[test]
    fn missing_device_id_creates_no_backup() {
        let dir = temp_dir("missing");
        let path = dir.join("device-id.txt");
        assert!(load_device_id_from(&path).is_none());
        assert!(!dir.join("device-id.txt.corrupt").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn mon(x: i32, y: i32, w: i32, h: i32) -> Monitor {
        Monitor { x, y, w, h, name: String::new() }
    }

    /// monitors の値域: 境界値(1・32768・-32768)は通し、1 つでも外れる値
    /// (0・負の幅・32769・座標が ±32768 を超える)は不正
    #[test]
    fn monitor_valid_accepts_screen_range_boundaries() {
        assert!(monitor_valid(&mon(-32768, 32768, 1, 32768)), "全て境界値でも有効");
        for bad in [
            mon(0, 0, 0, 1080),      // 幅 0
            mon(0, 0, 1920, -1),     // 負の高さ
            mon(0, 0, 32769, 1080),  // 幅が上限超え
            mon(0, 0, 1920, i32::MAX),
            mon(-32769, 0, 1920, 1080), // x が下限未満
            mon(0, 32769, 1920, 1080),  // y が上限超え
            mon(0, 0, 1920, i32::MIN),
        ] {
            assert!(!monitor_valid(&bad), "値域外は不正: {bad:?}");
        }
    }

    /// sanitize_monitors: 不正な枚を除外し、件数は MAX_MONITORS で止める。
    /// 全滅なら空配列(単一画面扱い)に落ちる
    #[test]
    fn sanitize_monitors_drops_invalid_and_caps_count() {
        let good = mon(0, 0, 1920, 1080);
        let bad = mon(0, 0, 0, 0);
        // 不正混じり: 正しい物だけ残る
        let got = sanitize_monitors(&[good.clone(), bad.clone(), good.clone()]);
        assert_eq!(got.len(), 2);
        // 件数上限: 10 面送られても 8 面
        let many = vec![good.clone(); 10];
        assert_eq!(sanitize_monitors(&many).len(), MAX_MONITORS);
        // 全滅: 空配列(= hello の単一画面フォールバックと同じ状態)
        assert!(sanitize_monitors(&[bad.clone(), bad]).is_empty());
        // 空入力は空のまま
        assert!(sanitize_monitors(&[]).is_empty());
    }
}
