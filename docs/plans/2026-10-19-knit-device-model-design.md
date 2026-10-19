# Knit 端末モデル(端末別認証・登録台帳・失効)設計書

作成: 2026-10-19 / 根拠: 3断面探査+統合アーキテクチャ(2026-10-19 セッション内)、要件 output/2026-09-27_tsunagu-product-requirements/requirements.json
状態:起草中。§0・§3.1・§4-C は arena 統合後に確定する【arena 確定待ち】と明示した箇所だけ空けてある。

## 0. 先に結論(3行)

1. 端末別資格は「master(既存トークン)から `Blake2s256("knit-device-v1\0"+master+"\0"+key_id)` で導く64桁hex の端末別トークン」。Noise・プロトコルメッセージ・Win の保存経路は1バイトも変わらない(arena 候補1を base)
2. accept は最初の Noise レコードを台帳の全鍵で試行し、どの鍵で通ったか(`Who = Legacy | Device(key_id)`)を返す。失効は「台帳から鍵が消えること」そのもので、本線・音声・bulk の全経路に同時に効く
3. 旧トークンは自動的に legacy 扱いで共存する(全再登録不要)。`revoke_legacy` で旧形式だけを閉じられる。将来の静的鍵化(IKpsk2)は snow 0.10 で動作実証済みで、台帳の型(DeviceKeyId・Who)はそのまま使える

## 1. 背景と目標

現状の信頼モデルは「全端末で同一の 256bit 共有トークン1本」。登録(pairing)は SPAKE2/SAS で認証した経路で**同じトークンを配るだけ**(pairing.rs:397-421)。したがって:

- サーバは接続者がどの端末かを鍵から区別できない(Noise NNpsk0・PSK は1本。hello の id は認証後の名乗り)
- 登録台帳がなく、失効は「全登録の初期化」=全端末締め出ししかない
- 未払いのセキュリティ指摘 M2(端末単位失効)が解けない(improvement-log.md:491 がこの設計作業に先送り記録済み)

対象要件(すべて status=提案・未承認。承認済みと偽らない):
- **G02(核)**: 端末固有の鍵で認証。登録解除で通信と操作権を止める。漏えい時は端末単位で失効
- **M2(未払い債務)**: 共通トークンの端末単位失効
- **G01(接続)**: 登録時に端末識別を信頼の一部にする(名前・IP 変更後も同一端末)
- **M02/M03(後段)**: 操作権の状態機械。**今回の設計には含めるが実装は今回外す**

## 2. 測定可能な受け入れ基準(反証可能な述語)

| # | 基準 | 証明方法 |
|---|---|---|
| AC1 | `cargo test --workspace` が緑。かつ新規テストを含む件数が現行(611)以上 | CI(macos+windows)とローカル |
| AC2 | **失効の実演**: 端末Xを登録→Xを失効→Xの再ハンドシェイクが拒否され、端末Yは登録・接続・通信を続けられる | ループバックの自動テスト(実機を前提にしない) |
| AC3 | **後方互換**: 現行形式(共通トークン・版17クライアント)が新サーバへ登録・接続できる。また旧サーバは新クライアントを版互換の範囲で受け入れる | 旧形式メッセージを使った往復テスト |
| AC4 | **冪等性**: 失効を2回実行しても冪等。途中で落ちても台帳が壊れない(原子保存) | 失効二回・kill 想定の単体テスト |
| AC5 | 設計書(本文書)が増分・やらないこと・弱点を明記して完了している | レビュー |
| AC6 | 決定ログ(decisions.tsv)が各増分を行単位で記録し、PR から追跡できる | PR リンク |

## 3. 設計の柱と、層の分け方

### 3.1 端末別認証の核(arena 統合形・base=候補1)

採用形の全体像(型・シグネチャの全文は [arena/candidate-1-inheritance.md](./2026-10-19-knit-device-model-arena/candidate-1-inheritance.md) を正本とする):

**導出。** `device_token(master, key_id) = hex(Blake2s256("knit-device-v1\0" + master + "\0" + key_id))`。常に64桁hex なので既存 `parse_key` を通る(境界検証は既存関数が担う)。ドメイン分離は psk("knit-psk-v1")・fingerprint("knit-fingerprint-v1")・room_id("knit-room-v1") と同じ作法。

**鍵束と accept。** `secure::ServerKeys`(中身は非公開の Vec<(Who, PSK)>。空では作れない)に `single(token)`(旧運用・Win・テスト)と `legacy_plus(master, devices)`(Mac)の2コンストラクタ。`accept(stream, &keys, label)` は最初の Noise レコードを1回だけ読み、鍵ごとに responder の HandshakeState を作り直して `read_message` を試す(snow 0.10 は message が借用渡し・失敗は Err のみでこの形が成立することを判定者が snow ソース handshakestate.rs:336-347 で裏取り済み)。**TCP 往復は増えない**(クライアントから見て現状と同一バイト列)。全滅なら現状どおり PermissionDenied+切断。成功した鍵の `Who` を返す。鍵数は cap 64。

**台帳。** `DeviceRegistry`(crates/common/src/registry.rs 新)。devices.json(非秘密・write_atomic)に key_id・name・fp・added_at・**last_seen**(候補2から移植)。master も導出トークンも書かない(漏えても鍵は導出不能)。`enroll(name)` は常に新規 key_id 採番・"saved" ack 前にコミット(ack 欠落で二度と繋がらなくなる事故の防止。未使用エントリは UI から掃除)。`revoke(key_id)` は物理削除で冪等(NotFound も成功)。`revoke_legacy()` は旧形式の受付停止(devices.json の legacy_revoked に永続)。`note_seen` は接続成功時のみ記録(失効後の試行は記録しない=列挙の手がかりを出さない)。UI の「登録済み: N台」は台帳からのみ出す(二重管理しない)。

**宛先解決。** Mac が接続側になる経路(client_attempt・bulk connect_loop)は `outgoing_token(peer_device_id)`。未紐付けの宛先(旧端末)は master をそのまま返す。discover は Mac の応答側だけ `discover_rooms()`(room_id(master)+全 room_id(導出トークン))にマルチルーム化。Win の seek 側・Win の応答側(KNIT_ROLE=server)は単一 room のまま。

**後段への境界。** `PeerEntry.auth: Who` が認証済み主体の運び場所。許可行列・操作権リースはこの1フィールドから引く。

### 3.1.5 arena の統合記録

- base: 候補1(継承)。cross-judge は6規準で 29対24。リードの独立通読も同じ結論(後方互換の自動維持・全変更が common+mac・Win 差分実質3行)
- 候補2から移植: ①last_seen と L03 の扱い(失効後に接続試行が無いことを UI が示せる) ②タイミング漏れの Tradeoffs 明記と回帰テスト ③登録境界の不変条件(候補1は常に新規採番のため「同一鍵の再登録拒否」は構造上不要。**失効が新しい登録を跨いで持続する**ことの固定テストとして採用)
- 棄却した候補2の主要部: IKpsk2・レコード長ディスパッチ・AuthUpgrade・ClientCredential。理由は v11..v17 との相互接続に4機構の組合せ証明が要ること・win/android にローカル検証不能なコードが置かれること・connect のシグネチャ変更が35箇所に波及すること
- 将来道として記録: IKpsk2(静的鍵)は snow 0.10 で名前解決・2メッセージ・get_remote_static・第三者鍵の真正開示まで実証済み(/tmp/snowcheck で exit 0)。台帳の DeviceKeyId を公開鍵の指紋に差し替える形で移行できる
- 検証: snow IKpsk2 実証(リード)・全鍵試行の実装可能性(snow ソース裏付け・判定者)・待受4箇所と Mac 接続側2経路の実在(判定者が file:line で確認)

### 3.2 登録台帳(新設・共通モジュール)

- **台帳は秘密ではない**(端末ID・表示名・鍵の世代・失効状態)。data_dir の JSON に原子保存(persist::write_atomic)。既存パターン: peer-sides.json・screen-settings.json と同じ「id キーの JSON ストア」
- **秘密は OS 保護へ**(Keychain/DPAPI)。台帳には鍵の参照(どのスロットか)だけ置く
- 台帳の操作(登録・失効・一覧)は純関数+薄いIO の分层けにし、ループバックテストを書きやすくする
- PeerEntry(ランタイム)と台帳(永続)は**別物**。台帳が真実で、PeerEntry は接続の写像

### 3.3 操作権(M02/M03・後段設計・今回実装外)

統合アーキテクチャの Extension Anchors に基づく設計方向だけ記す:

- 操作権の実体は今日 ACTIVE_PEER+WIN_MODE / CONTROLLED の2フラグで暗黙。これを「リース」にする: 活性化(activate_peer_locked/Warp)で期限付きの権利を付与し、生存監視(ping 1秒/pong 15秒)をリース更新に昇格。pong 途絶=リース失効=leave(押下解放は既存の Leave→release_everything 経路をそのまま使う)
- 受信側(Win)の CONTROLLED をフラグから状態機械へ拡張し、Warp/Leave を「要求→許可/拒否(理由付き)」へ(SecureChannel でなく Msg の新行。Unlock(v16)の作法=版ゲートで送らない・旧側は未知行を無視、に倣う)
- 入力の世代(OUTBOUND_GENERATION)は M03 の「古い入力を再送しない」を既に半分実現している。リース失効で世代を進めれば完全化する
- 競合(M02)は現行「1台だけアクティブ」の構造を維持し、逐次切替(世代破棄)で解く。同時押しの合成は**やらない**(§7)

## 4. 増分リスト(依存順・1増分=1コミット)

| 増分 | 内容 | 主な触る場所 | 検証 | 依存 |
|---|---|---|---|---|
| A | 台帳モジュール。DeviceKeyId・device_token 導出・DeviceRegistry(open/enroll/revoke/revoke_legacy/note_seen/devices)・devices.json の原子保存と非漏えい | crates/common/src/registry.rs(新) | 単体テスト: 冪等(revoke 2回で台帳バイト同一)・shape(64hex・ドメイン分離)・台帳 JSON に master/トークンが部分文字列として現れない・破損時退避 | なし |
| B | secure の鍵束。ServerKeys(single/legacy_plus・非空保証・cap64)・全鍵試行 accept・Who | crates/common/src/secure.rs | ループバック: single で旧どおり・legacy_plus で Device/Legacy 判別・全滅時 PermissionDenied・ラベル取り違え拒否・タイミング回帰(テスト6) | A |
| C | 登録の発行と Mac 配線。pairing serve_credential が enroll・mac session/audio の accept 差し替え・PeerEntry.auth・outgoing_token・bulk・discover_rooms | common/pairing.rs・bulk.rs・discover.rs + crates/mac | pairing ループバック(テスト4: 受け取る token が導出鍵で accept に通る)・three_paths(テスト2) | B |
| D | 失効の実演と操作経路。**AC2 の主軸テスト**(登録A/B→revoke(A)→A拒否・B無影響・legacy無影響・冪等)・稼働セッション切断(待機はLeave後切断・アクティブはdrop_stream)・操作経路 --registry CLI(設定 GUI の一覧は後続増分)・Win 差分はCで済み | common(tests/device_registry.rs)+mac(conn/main) | AC2+AC3+AC4 の全部・CI 両 OS・実バイナリで --registry list を実証 | C |

各増分のコミット前に `cargo test --workspace` を緑で通す(sequence-verifiable-units)。Windows 側を触る増分は CI の windows-latest を通してから次へ。

## 5. Windows 側が必要な作業(ローカル検証不可・CI 依存)

- 増分B: client_credential の受取側(端末別資格の保存。DPAPI 複数スロット化)
- 増分C: 接続時の鍵ヒント送出(クライアント側の変更は接続1箇所のはず)
- 増分D: 失効された側の表示(切断理由の文言)
- この Mac では Windows ターゲットをコンパイルできないため、これらは CI 通過をもって最初の検証とする。実機確認は残タスクとして明記する

## 6. 壊してはいけない既存の良さ(統合アーキテクチャの Gotchas から)

1. **版ゲートの一元化**: 呼び出し側に `ver >= 18` 等のリテラルを書かせない。peer_features に集約する(proto.rs:32-55)
2. **serde(default) の作法**: 新フィールドは必ず default 付き。未知の行は受信側が黙って無力(無視)する前提は崩さない
3. **「未接続=版0」の不変条件**(両側で切断時に版を0に戻す)
4. **FailThrottle の適用**: 鍵選択の失敗も throttle の対象にする(成否オラクルの鈍化)
5. **fingerprint/psk/room_id のドメイン分離**(knit-psk-v1 / knit-fingerprint-v1 / knit-room-v1)。新導出にも別ドメインを与える
6. **原子保存**(write_atomic / MoveFileExW)と**上書き拒否**の作法
7. **登録の安全装置**: 6桁コード使い捨て・試行3回・壁時計期限・ビーコンだけの偽装排除・SAS の commitment
8. **room_id が既にトークンの安定ハッシュを LAN に晒している**事実を踏まえ、新方式の漏えい面をこれより悪化させない

## 7. 実装しないと決めたこと(と、その理由)

1. **操作権リースの実装(M02/M03)**: 入力経路(tap.rs 3282行)を触る変更はリスクが大きく、今回の検証資産(ループバック)が効かない。§3.3 に設計を残し、端末別認証が着地してから別増分にする
2. **許可行列(機能×方向×端末)の実装**: 台帳にプレースホルダ(境界の名前)だけ置く。 enforcement は操作権と同じ後段。Android の TabletInfo control/keyboard が先例
3. **同時操作の入力合成(M02 の完全形)**: 世代破棄による逐次切替で解く。合成は複雑度に見合わない
4. **My Desk・多対多の「机」概念(G01 の完全形)**: 接続方向の再設計を含み、規模が本次の範囲を超える。G01 は「登録時に id を信頼に接続する」まで
5. **Linux 対応・QR 登録**: 要件にはあるが本次の範囲外
6. **KNIT_TOKEN 単一運用の廃止**: 移行期間は二重受付で共存させる(§3.1 確定時に詳細)

## 8. 要件 → 増分の対応表

| 要件 | 今回 | 増分 |
|---|---|---|
| G02 端末別鍵・認証 | する | B・C |
| G02 端末単位失効(=M2) | する | A・D |
| G02 登録解除で通信と操作権を止める | 通信は本次。操作権は設計のみ | D + §3.3 |
| G01 登録時の端末識別 | する(突合の土台) | B |
| G01 全方向の机 | しない(§7-4) | — |
| M02 操作権の競合解決 | 設計のみ | §3.3 |
| M03 期限と安全な復帰 | 設計のみ(入力世代は既存) | §3.3 |
| L03 届かない失効を成功としない | 文言と台帳の状態で誠実に | D |

## 9. 設計の弱点・未検証の前提(正直に)

1. **失効は次回接続まで届かない**: オフラインの失効済み端末との既存接続を切れるが、端末がオフラインなら「拒否」は次のハンドシェイクでしか起きない。L03 の「届いていない削除要求を成功としない」は UI 文言で誠実に扱う(last_seen で「失効後に一度も接続していない」を示せる。候補2から移植)。テクニカルには失効リストの配布も考えたが、複雑度に見合わないと判断
2. **タイミング漏れ(graft)**: 全鍵試行の線形探索は「どの鍵で当たったか」を処理時間で漏らす素地がある。鍵数 cap 64 と FailThrottle(接続単位)で軽減するが除去はしない。テスト6で鍵位置別の応答時間分散を固定し、悪化を検知できるようにする
3. **legacy(旧形式共有鍵)は revoke_legacy されるまで常設(判定者の留保)**: 移行完了を利用者の手動操作に頼る。UI に「旧形式での接続を受け付けています」を常時表示し、revoke_legacy へ導く。放置した部屋では M2 が解けたままの窓が残る
4. **master 漏えい = 全端末トークンの導出可能性**: 台帳を非秘密(JSON)に保つ利点と引き換えに受け入れる。Mac の Keychain を割られた時点で「新しい鍵を配る」権限も漏れているため実害の差は小さい。端末側の漏えいは導出トークン1本分に隔離される(逆算不能・他端末の key_id は不明)
5. **既存登録端末の移行**: 登録済みの Win 端末は旧トークン=legacy で自動共存するが、**失効の恩恵を受けるには再登録が必要**(再登録は新 key_id 採番)。UI の一覧に legacy 接続中の端末を「旧形式」と見分けられる表示を要する
6. **実機2台での登録 UX は未検証**: ループバックテストが上限。実機確認手順を残タスクとして書く
7. **Android の Keystore 保存**: android-bridge は validate_transport_key(32..=512)で導出トークンを保存可能。実機での discover・音声結合は今回の検証範囲外
8. **Mac が本線クライアントになる経路は master のまま**: client_attempt/client_thread(接続先が誰か hello の前に分からない)は宛先別トークンを解決できないため master を使う。台帳端末が相手だと本線のハンドシェイクに失敗する(bulk 接続側はアクティブ宛先の解決済み)。役割反転運用は実験的位置づけであり、後続の「机」設計で解く
9. **設定 GUI の一覧・失効は後続増分**: 今回の操作経路は knit-mac --registry(list / revoke / revoke-legacy)。本体起動中でも効く(台帳は mtime で再読込)。見た目の整備と「旧形式を受け付け中」の常時表示は UI 増分で載せる

## 10. 実装の順序(まとめ)

1. 増分A(台帳の型)→ 2. B(発行と保存)→ 3. C(鍵選択・arena 確定形)→ 4. D(失効の実演)→ 5. 実機確認手順の整備+報告書。各増分は WIP ブランチに 1コミット、CI 緑を確認して次へ。

## 11. 実装状況(追記用)

- 2026-10-19: 起草。§3.1/§4-C は arena 統合待ち
- 2026-10-19: arena 完了(base=候補1・graft3件・judge 29対24)。§0/§3.1/§4 を確定。snow IKpsk2 の動作実証と全鍵試行の実装可能性裏取り済み。成果物は [arena/](./2026-10-19-knit-device-model-arena/)
- 2026-10-19: **増分A〜D 着地**(PR #13・ブランチ wip/device-model)。AC1〜AC4 まで実証済み(ループバック+ワークスペース全緑+実バイナリの --registry 動作+CI 両 OS)。AC5 は本設計書。**残タスク**: 設定 GUI の一覧・失効・「旧形式を受け付け中」表示 / 実機2台での登録・失効の確認 / M02・M03(操作権リース)の実装 / Mac がクライアントになる経路の宛先別トークン(§9-8)
