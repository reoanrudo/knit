# LAN 直優先と Tailscale フォールバック(設計)

日付: 2026-09-26
ステータス: 承認待ち(ブレインストーミング: ユーザーがアプローチ A へ切り替えを承認。
先行する B「RTT 品質監視」設計は不採択)

## 背景

- 2 台(Windows=192.168.0.2 有線、Mac=192.168.0.1 無線)は同一ルーター下にあるが、
  tsunagu の全経路は Tailscale IP(100.x)経由で張られている。Tailscale の直結 UDP が切れて
  DERP 中継へ落ちると、接続は維持されたまま遅延が数倍になる(ユーザーの困りごと)
- 実測(`tailscale status`): 現在 `direct 192.168.0.2:41641` —— Tailscale が健康な時の
  実体は既に LAN 直接であり、**Tailscale を経路から外しても良い時の品質は変わらない**。
  AP アイソレーションは「直結が成立している」ことで否定済み(LAN 直も通る)
- セキュリティは v0.23.0 の Noise(NNpsk0・トークン相互認証)+ `net::is_allowed`
  (LAN・リンクローカル・Tailscale を許可)で独自に成立しており、Tailscale に依存しない

## 目的

- 同一 LAN 内では **Tailscale の状態と無関係に常時 LAN 直**で接続する(中継劣化の原因を消す)
- 外出先等 LAN 外からの利用では Tailscale へ自動フォールバックする
- プロトコル変更・`MIN_VERSION` 繰り上げをしない(片側だけの配備でも既存接続が壊れない)

## 設計

### 1. Mac: 待受を全インターフェースへ(1 行)

- `scripts/restart-mac.sh` の `BIND_IP=$(tailscale ip -4 …)` を `BIND_IP="0.0.0.0"` に変更する。
  本線 24900・音声 24901・bulk 24902 が全 IF で待受ける(コード変更なし。`TSUNAGU_BIND` の
  上書き経路は残す)
- LAN への露出は `is_allowed` による接続元絞り込み + Noise ハンドシェイク(不一致は切断)で
  防御する。UDP 発見応答(24903)は既に `0.0.0.0` 常駐で同じモデルであり、新たな暴露面は
  TCP 3 ポートのみ
- Mac の Tailscale 経路診断(30 秒ポーリング・通知)は参照情報として残す

### 2. Windows: 接続候補を「LAN 発見 ∪ TSUNAGU_HOST」へ統合

- `connect::resolve` を変更: `TSUNAGU_HOST` 指定の有無にかかわらず、
  LAN 発見の結果と指定ホストを**重複排除して併合**し、既存の `first_reachable`
  (同時レース・最初に繋がったもの採用)へ渡す。発見結果を先頭に並べる
- 発見に `discover::seek_first` を追加: 最初の応答またはタイムアウト(600ms)で返す。
  LAN 内では応答は数 ms で届くため実質の遅延増は小さく、遠隔(LAN 内に誰もいない)でも
  再接続 1 回あたり +600ms で済む
- Windows の `TSUNAGU_HOST` は Tailscale IP(100.100.10.9)のまま変更しない
  (=フォールバック候補)。Mac 側 IP のハードコードは不要になる
- `client_loop` は既に再接続のたびに `resolve` を呼び直すため、スリープ復帰・切断後の
  再接続で自動的に LAN 直が選ばれる(帰宅シナリオも次の再接続で解決)

### 3. 音声・bulk の追従(変更不要)

- 既存実装の `PEER` static(本線が選んだアドレス)に音声 24901・bulk 24902 の接続先が
  追従する。LAN 直を選べば 3 経路すべてが同じ LAN IP へ向かう

### 4. 設定・ドキュメント

- `TSUNAGU_HOST`(フォールバック)/ `TSUNAGU_BIND`(既定 0.0.0.0)の説明を docs/usage.md へ。
  CHANGELOG.md に記載

## テスト方針

1. 単体テスト(common): 併合ロジック(発見優先の順序・重複排除・指定無し時)、
   `seek_first` の早期終了とタイムアウト(ローカル UDP 応答スレッドを立てて検証)
2. `cargo test` の回帰(現行 15 件が壊れないこと)
3. 実機検証: 配備後、Windows ログが `connected (192.168.0.1:24900)` となり、
   `[bulk] established (→192.168.0.1:24902)`・音声も同 IP であることを確認。
   verify.sh 全項目 pass。Mac 側 `tailscale status` の経路表示と照合

## 非目標(YAGNI)

- RTT 計測・劣化判定・再接続トリガ(B 設計。同一 LAN では LAN 直で根本解決するため不採択)
- セッション確立中の経路昇格(張り替えは次の再接続機会に任せる)
- 複数経路の同時保持(アクティブ・スタンバイ)
- 音声・bulk の経路個別選択

## 対象ファイル(想定)

- `scripts/restart-mac.sh`: BIND_IP の既定を 0.0.0.0 へ(1 行)
- `crates/common/src/lib.rs`: `connect::resolve` の併合化 + `discover::seek_first` + 単体テスト
- `crates/win/src/main.rs`: 呼び出しの調整(あれば。基本は common 側で完結)
- `docs/usage.md` / `CHANGELOG.md`

## 配備に関する特記事項

- プロトコル変更が無いため、Mac・Windows の更新順序は自由(バインド拡大は既存の
  Tailscale IP 宛接続も受け続ける)。通常どおり restart-mac.sh --diag → deploy-win.sh →
  verify.sh を想定
