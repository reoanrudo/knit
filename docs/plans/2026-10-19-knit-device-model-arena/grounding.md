# Arena グラウンド情報: Knit 端末別認証と登録台帳(2026-10-19)

リポジトリ: /Users/taguchireo/Documents/八幡平市地域おこし協力隊/05_個人/knit
この文書は how スキルによる3断面探査(信頼・セッション・許可/操作権)の統合結果と、制約の宣言。設計候補者はまずこれを読み、必要ならコードを自分で検証すること(引用はすべて現行コードで検証済み)。

## 0. 設計課題(タスク)

現在の Knit の信頼モデルは「全端末で同一の 256bit 共有トークン1本」。登録は認証済み経路でその同じトークンを配るだけ。よって:

- サーバは接続者が「どの端末」かを鍵から区別できない(Noise NNpsk0、PSK=Blake2s256("knit-psk-v1\0"+token))
- 登録台帳がなく、失効は「全登録の初期化」(全端末締め出し)しかない
- 未払いのセキュリティ指摘 M2(端末単位失効)が解けない

設計すべきもの: **端末別の資格発行・保存・ハンドシェイク時の端末特定・登録台帳・端末単位の失効**。型スケッチ・シグネチャ・モジュールmap・usage-first の根拠書を出すこと。実装はしない。

## 1. 目標要件(出典: output/2026-09-27_tsunagu-product-requirements/requirements.json。status はすべて「提案・未承認」)

- G01 端末登録: どの対応PCからも机を作成・参加できる。短いコードまたはQRと相手確認で登録する。名前変更・IP変更後も同じ端末と識別する
- G02 信頼と共有権限: 端末固有の鍵で認証し、端末・機能・方向ごとに許可する。探索された端末やIPを信頼の根拠にしない。登録解除で通信と操作権を止める。漏えい時は端末単位で失効し、他端末を再登録させない
- M02 複数端末からの操作: 受信側が操作権を管理し、同時操作の競合を解決する。相手が操作中なら理由を示し、無断で奪わない
- M03 安全な復帰: 押下状態と操作権に期限を持たせる。切断・ロック・権限取消で操作を手元に戻し、押下を解放する。古い入力は再送しない
- M2(セキュリティ指摘・未払い): 共通トークンの端末単位失効。improvement-log.md:491 が「端末モデルの設計作業で対応」と先送り記録済み
- L03(参考): 紛失端末を失効し、オフライン端末に届いていない削除要求を成功としない

今回の設計の核は G02 の「端末別鍵+台帳+認証されたid」と M2。M02/M03 は後段の操作権設計に接続する点のみ考慮。

## 2. 現状アーキテクチャ(統合済み・file:line は現行コードで検証済み)

### 信頼

- credentials.rs:62-66 — generate: 32バイト乱数→64桁hex。generate_shared_token(70-116): KNIT_TOKEN 用32文字(約190bit)
- credentials.rs:117-127 — 単一スロットの load/save/delete。mac.rs:47-74 は Keychain(service="local.knit.connection", account="pairing-token-v1")、削除は errSecItemNotFound を冪等に成功(121-131)。win.rs:149-184 は CryptProtectData→%LOCALAPPDATA%\Knit\connection.dpapi へ一時ファイル+MoveFileExW(WRITE_THROUGH) で原子保存、上書き拒否(AlreadyExists)
- credentials.rs:214-233 — save_peer/load_peer: 非秘密の再接続ヒント paired-host.txt。書くのは Windows 側のみ(win/tray/setup.rs:187,237)
- secure.rs:15-21 — psk(token) = Blake2s256("knit-psk-v1\0"+token)。secure.rs:221-254 — connect/accept は Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s + 経路別 label(knit-main/knit-audio/knit-bulk)。トークン不一致はここで失敗(成否がオラクルになるため短いトークンは危険、mac/main.rs:827 コメント)
- secure.rs:27-34 — fingerprint = SHA-256("knit-fingerprint-v1\0"+token) 先頭8桁。表示専用・ドメイン分離
- secure.rs:259-286 — FailThrottle(連続失敗5回目から1失敗250msずつ上限5秒、成功で即回復)。mac/session.rs:545・win/conn.rs:372・mac/audio.rs:1402・bulk.rs:1376 の4待受で適用
- pairing.rs:159-321 — Invitation::bind/open(TCP+UDP 24904・6桁コード・壁時計300秒・試行3回)。serve_one(356-394)は SAS_MARK なら承認方式、他は6桁+SPAKE2
- pairing.rs:494-552 serve_sas / 556-625 request_approval — commitment(BLAKE2s "knit-sas-commit-v1")+X25519+4桁確認番号(4択に偽物混入 431)。低位数点拒否(455)
- pairing.rs:629-685 — enroll 系(Spake2 Ed25519Group、id=b"knit-windows-v1")
- pairing.rs:397-421, 687-713 — serve_credential/client_credential。**渡す token は常に Mac の単一共有トークンそのもの**。{token, port} の JSON を、PAKE鍵を PSK にした Noise NNpsk0(prologue "knit-enrollment-v1")で渡す(Channel::handshake 817-852)
- pairing.rs:916-973 — discover/ビーコン。TCP 24904 開存確認で候補化(ビーコンだけの偽装排除)
- net.rs:23-99 — is_allowed(既定 private/link-local/loopback。Tailscale は KNIT_ALLOW_TS=1)。pairing accept・ビーコン・本線・音声・bulk・discover の全入口で適用
- discover.rs:12-21 — room_id = Blake2s("knit-room-v1\0"+token) 先頭8バイト。UDP 24903。同じトークンの相手だけ応答(トークン自体は流さない)
- トークン読み込み: envutil(環境変数 > 実行ファイル同階層 .env > ~/.config/knit/env。旧名 TSUNAGU_/SEAMLESS_ も)。mac/main.rs:829-846・win/main.rs:394-401 — KNIT_TOKEN(≥32文字)があれば最優先、次に credentials::load()、無ければ初回登録UI。32文字未満は fatal
- mac/gui/setup.rs:83-98 — Mac はコード表示前に鍵を keychain へ保存。追加登録(392)は既存鍵を使い回し。登録は1成功で招待終了・コード使い捨て(pairing.rs:281-284)
- setup.rs:413-470・win/tray.rs:861 — 「すべての登録を初期化」= credentials::delete+generate+save(全端末締め出し)。**1端末だけの削除経路は存在しない**
- KNIT_TOKEN 運用は UI から初期化不能(setup.rs:418-423)

### セッション・プロトコル

- proto.rs:21-28 — VERSION=17/MIN_VERSION=11/compatible。版履歴コメント(4-20): 13=hello に id+monitors、14=フォルダ+BLAKE2s+mtime、15=XferAck・RoleAck・Cfg の files/listen ビット、16=Unlock、17=drag.lazy Manifest
- proto.rs:35-55 — peer_features(ver)->PeerFeatures{dirs,empty_dirs,unlock,lazy}。**版ゲートの唯一の集約点**。呼び出し側に閾値リテラルを書かせない方針
- proto.rs:132-164 — device_id(): device-id.txt に永続(原子保存、壊れれば .corrupt 退避のうえ再生成、テスト 518-565)。**暗号用途ではなく UI 紐付け専用の自己申告ID**
- proto.rs:196-215 — safe_peer_name(制御文字・Bidi 除去+48文字)
- proto.rs:228-477 — Msg enum。1行=1 JSON タグ付き。未知の行は decode() が None→旧側は黙って無視。**新フィールドは必ず #[serde(default)]**(テスト 716-723)。Cfg.target_range は skip_serializing_if も併用、clip/files/listen は default_true
- mac/conn.rs:69-95 — PeerEntry(id/name/ip/screen/monitors/writer/gen/side/edge_monitor/ver/alias)。96-102 — PEERS/ACTIVE_PEER/PEER_GEN/PEER_CHANGES。ロック順 STREAM_SLOT→PEERS→ACTIVE_PEER(296)
- mac/peers.rs:9-27 insert(同id再接続は side/alias 継承・旧 writer shutdown)/29-55 detach(id+gen 一致のみ)/63-65(全台消滅時だけ peer-sides.json 保存)
- mac/conn.rs:344-451 — activate_peer_locked(切替フル処理)。457-540 — keepalive_inactive_peers(1秒周期)
- mac/session.rs:60-336 — session_receive_loop。受信一本化: id+gen 不一致は廃棄(82-84)、非アクティブは Bye/TabletInfo/Screen/Ping のみ受理(85-87)
- mac/session.rs:403-477 — handshake_and_hello。ver 検査・名前無害化・旧版(id空)は legacy-{ip}(456-461)。グローバル PEER_VERSION はここで書かない
- mac/outgoing.rs:3-14 — Queued{generation}。OUTBOUND_GENERATION 不一致の入力は writer で破棄(旧端末への入力が次端末へ再生されるのを防ぐ)
- win は単一接続(conn.rs:128 PEER: Option<IpAddr>)。版は dragdrop/edge.rs:17 PEER_VERSION: AtomicU32。hello/hello_ok 受信で store、**切断時に 0**(「未接続=版0」の不変条件、両側対称)
- mac/session.rs:491-707 server_thread — accept→is_allowed→HANDSHAKE_PENDING≤4・15秒期限→セッションスレッド spawn
- 音声 24901・bulk 24902 も同じ PSK の別 label。bulk は Endpoint ハンドシェークで token を使う(bulk.rs 1370-1480 付近)

### 許可・操作権(今回の設計への接続点のみ)

- share.rs:14-19 Scope{clip,files,audio}(input は常に共有で Scope の外)。KNIT_SHARE env 上限∧ユーザートグル(114-120)。**端末別の許可は存在しない**
- proto.rs:386-410 Msg::Cfg — clip/files/listen/spk_mute。Mac が送る clip と Mac が聞く listen が別ビット=事実上の方向表現
- 操作権の実体: ACTIVE_PEER+WIN_MODE(mac/state.rs:7, conn.rs:98)と CONTROLLED(win/dragdrop/edge.rs:18)。リース・期限・競合裁定なし
- Android 前例: proto.rs:350 TabletInfo{control,keyboard,japanese}(端末が自己申告)→ mac/android/app.rs:5-32 のプロセス内 HashMap(非永続)。control=false は境界越え拒否+強制 leave(session.rs:297-299, tap.rs:1315-1323)
- idキーJSONストアの既存パターン: peer-sides.json(mac/tap.rs:752-772・エイリアス798-824)、screen-settings.json(mac/screen_settings.rs:73-78, BTreeMap<String, PeerSettings>)

### Gotchas(設計が踏むべきでない地雷)

1. hello の id は認証されない。トークンを持つ全員が全員になりすませる(G02/M2 の根本障害)
2. 「未接続=版0」は両側の不変条件。版ゲートは 0 で必ず閉じる
3. device-id.txt が壊れると id 紐付けが全て切れる(.corrupt 退避のうえ再生成)
4. activate_peer_locked は WIN_MODE を触らない(切替と操作権が直交)
5. Leave は CONNECTED=true のときしか送らない。経路断時の相手側押下回収は pong 15秒/9秒検知に依存
6. keepalive 1秒は Win「9秒無通信で切断」への余裕。writer は一時 take で切替と競合(2秒待機で解消)
7. 経路昇格(Tailscale→LAN)で一時的に2本張る。世代番号(PEER_GEN)で新セッションを正とし旧は静かに退出
8. room_id はトークンの安定ハッシュを LAN に晒す(同じ鍵の組の特定には使える)

## 3. ハード制約(設計が満たすべきもの)

1. **後方互換**: 現行 v11..v17 の相手との相互接続を壊さない。作法は「新フィールドは serde(default)・未知の行は受信側が黙って無視・送る側が peer_features 版ゲートで守る」
2. **移行**: 既存の単一トークン運用(KNIT_TOKEN 直接接続・登録済みの既存端末)からの移行経路を設計に含める。全再登録を強制するならその理由と案内を書く
3. **Windows 側の差分は最小に**: この Mac では Windows ターゲットをコンパイルできない(CI の windows-latest 依存)
4. **laziness**: 最小差分。既存パターン(idキーJSONストア・peer_features 版ゲート・FailThrottle・write_atomic 原子保存・fingerprint のドメイン分離)を踏襲する。抽象を足さない
5. **検証可能性**: 「登録→失効→拒否・他端末は影響なし」をループバックの自動テストで証明できる設計であること(実機2台は前提にしない)
6. 秘密鍵の保存は OS 保護(Keychain/DPAPI)の既存パターンを踏襲。平文の鍵をディスクに置かない
7. 音声(24901)・bulk(24902)・登録(24904)の全経路で同じ資格体系が働くか、明示的に書く
