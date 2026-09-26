# LAN 直優先と Tailscale フォールバック 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 同一 LAN 内では Tailscale の状態と無関係に常時 LAN 直で接続し、LAN 外では TSUNAGU_HOST(Tailscale)へ自動フォールバックする。

**Architecture:** Windows の接続候補解決 `connect::resolve` を「LAN 発見 ∪ TSUNAGU_HOST」の併合へ変える(発見結果を先頭・重複排除)。発見には最初の応答または 600ms で返す新関数 `discover::seek_first` を使う。Mac 側は待受を 0.0.0.0 へ広げるだけでコード変更なし。音声・bulk は本線が選んだアドレス(PEER)へ追従する既存実装のまま。

**Tech Stack:** Rust(snow / blake2 は既存利用のまま変更なし)、bash(restart-mac.sh)。

## Global Constraints

- 設計書: `docs/superpowers/specs/2026-09-26-lan-direct-design.md`(承認済み)
- **プロトコル変更・`MIN_VERSION` 繰り上げをしない**(現行 11 のまま。片側だけ配備ても壊れない)
- トークンの値を画面・ログ・テストに出さない(テストではダミートークン文字列を使う)
- cargo を使う前に必ず `source $HOME/.cargo/env`(PATH 上の cargo には Windows ターゲット無し)
- 実機配備(scripts/restart-mac.sh / deploy-win.sh / verify.sh)はユーザーが実行。エージェントは実行しない
- コミット形式: 「改善ループN(第Nセッション): <要約>」+ 末尾に Co-Authored-By 行(N は git log の最新 +1)
- コメントは「なぜそうするか」のみ日本語で書く(プロジェクト規約)

---

### Task 1: discover::seek_first(最初の応答またはタイムアウトで返す)

**Files:**
- Modify: `crates/common/src/lib.rs`(discover モジュール、588 行の `seek_lan` 直後)
- Test: 同ファイル末尾の `#[cfg(test)] mod tests` 内

**Interfaces:**
- Consumes: 既存 `discover::seek(target: SocketAddr, token: &str, wait: Duration) -> Vec<IpAddr>`、`discover::room_id(token: &str) -> String`
- Produces(Task 2 が依存):
  - `pub fn seek_first(target: SocketAddr, token: &str, wait: Duration) -> Option<IpAddr>`
  - `pub fn seek_first_lan(port: u16, token: &str) -> Option<IpAddr>`(wait は 600ms 固定)

- [ ] **Step 1: 失敗するテストを書く**

`crates/common/src/lib.rs` の `mod tests` 内(`bulk_rejects_oversized_frames_and_overflowing_data` テストの後)へ追加:

```rust
    /// seek_first: 応答があれば即座に返り、無ければタイムアウトで None を返す。
    /// ループバックで応答側ソケットを立てて実動作を確認する
    #[test]
    fn seek_first_returns_first_answer_and_times_out() {
        use super::discover::{room_id, seek_first};
        use std::net::{SocketAddr, UdpSocket};
        use std::time::{Duration, Instant};
        let token = "seek-first-test-token";

        // 応答側を立てて、そのポートへ問い合わせる
        let responder = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = responder.local_addr().unwrap().port();
        let ask = format!("TSUNAGU?{}", room_id(token));
        let ans = format!("TSUNAGU!{}", room_id(token));
        let answerer = std::thread::spawn(move || {
            let mut buf = [0u8; 128];
            let (n, from) = responder.recv_from(&mut buf).unwrap();
            assert_eq!(&buf[..n], ask.as_bytes());
            responder.send_to(ans.as_bytes(), from).unwrap();
        });
        let t0 = Instant::now();
        let found = seek_first(SocketAddr::from(([127, 0, 0, 1], port)), token, Duration::from_secs(3));
        assert!(found.is_some(), "応答があるのに None");
        assert!(t0.elapsed() < Duration::from_secs(2), "応答があるなら長く待たない: {:?}", t0.elapsed());
        answerer.join().unwrap();

        // 応答が無ければ wait 経過で None。quiet は保持したまま(閉じたポートへ送ると
        // ICMP unreachable が Err として即座に返り、タイムアウト計測が崩れる環境がある)
        let quiet = UdpSocket::bind("127.0.0.1:0").unwrap();
        let quiet_port = quiet.local_addr().unwrap().port();
        let t0 = Instant::now();
        assert_eq!(
            seek_first(SocketAddr::from(([127, 0, 0, 1], quiet_port)), token, Duration::from_millis(300)),
            None,
            "応答が無いのに Some"
        );
        assert!(t0.elapsed() >= Duration::from_millis(250), "タイムアウト前に返った: {:?}", t0.elapsed());
        drop(quiet);
    }
```

- [ ] **Step 2: テストを実行して失敗を確認**

```bash
source $HOME/.cargo/env && cargo test -p tsunagu-common seek_first
```

期待: コンパイルエラー(`seek_first` が未定義)

- [ ] **Step 3: 最小実装**

`crates/common/src/lib.rs` の discover モジュールへ、`seek_lan`(588 行)の後に追加:

```rust
    /// 問い合わせ側(早期終了版)。最初の応答が届いた時点で返る。
    /// 再接続のたびに呼ばれるため、LAN 内の実質レイテンシは応答 1 往復分で済む
    pub fn seek_first(target: SocketAddr, token: &str, wait: Duration) -> Option<IpAddr> {
        let Ok(sock) = UdpSocket::bind(("0.0.0.0", 0)) else { return None };
        let _ = sock.set_broadcast(true);
        let ask = format!("TSUNAGU?{}", room_id(token));
        let ans = format!("TSUNAGU!{}", room_id(token));
        sock.send_to(ask.as_bytes(), target).ok()?;
        let until = Instant::now() + wait;
        let mut buf = [0u8; 128];
        // 不正な応答を受け取るたびにタイムアウトが延びないよう、残り時間を都度計算し直す
        while let Some(left) = until.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) {
            let _ = sock.set_read_timeout(Some(left));
            match sock.recv_from(&mut buf) {
                Ok((n, from)) if &buf[..n] == ans.as_bytes() => return Some(from.ip()),
                Ok(_) => {}
                Err(_) => return None,
            }
        }
        None
    }

    /// LAN 全体へ問い合わせ、最初に応答した相手を 1 つ返す。
    /// LAN 内なら応答は数 ms、誰もいなくても 600ms で諦める(再接続 1 回あたりの上乗せがこれ以下)
    pub fn seek_first_lan(port: u16, token: &str) -> Option<IpAddr> {
        seek_first(SocketAddr::from(([255, 255, 255, 255], port + PORT_OFFSET)), token, Duration::from_millis(600))
    }
```

- [ ] **Step 4: テストを実行して通過を確認**

```bash
cargo test -p tsunagu-common seek_first
```

期待: PASS(1 件)

- [ ] **Step 5: コミット**

```bash
git add crates/common/src/lib.rs
git commit -m "$(cat <<'EOF'
改善ループN(第11セッション): discover::seek_first(最初の応答 or 600ms)を追加

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: connect::resolve を「LAN 発見 ∪ TSUNAGU_HOST」の併合へ

**Files:**
- Modify: `crates/common/src/lib.rs`(connect モジュール、617 行の `resolve`)
- Modify: `crates/common/src/lib.rs`(discover モジュール、565-590 行の `seek` / `seek_lan` を削除 — 併合後は resolve からの参照が無くなるため。他の使用箇は grep 済みで存在しない)
- Modify: `crates/win/src/main.rs:1106`(候補ゼロ時のログ文言)
- Test: `crates/common/src/lib.rs` の `mod tests` 内

**Interfaces:**
- Consumes: Task 1 の `discover::seek_first_lan(port: u16, token: &str) -> Option<IpAddr>`、既存 `parse_hosts(list: &str, port: u16) -> Vec<SocketAddr>`
- Produces: `connect::resolve(hosts: Option<&str>, port: u16, token: &str) -> Vec<SocketAddr>`(シグネチャ不変。win/main.rs の呼び出し側はそのまま)。内部ヘルパ `merge_candidates(found: Vec<IpAddr>, hosts: Option<&str>, port: u16) -> Vec<SocketAddr>`(非公開・テストからのみ使用)

- [ ] **Step 1: 失敗するテストを書く**

`mod tests` 内(Task 1 のテストの後)へ追加:

```rust
    /// resolve の併合: 発見結果を先頭に、手動指定を重複排除して並べる
    #[test]
    fn merge_candidates_puts_discovery_first_and_dedups() {
        use super::connect::merge_candidates;
        use std::net::IpAddr;
        let found: Vec<IpAddr> = vec!["192.168.0.1".parse().unwrap()];
        let merged = merge_candidates(found, Some("100.100.10.9,192.168.0.1"), 24900);
        assert_eq!(merged.len(), 2, "発見と指定の同じ IP は 1 つに: {merged:?}");
        assert_eq!(merged[0].to_string(), "192.168.0.1:24900", "発見結果が先頭");
        assert_eq!(merged[1].to_string(), "100.100.10.9:24900");

        // 指定なし → 発見のみ
        let only = merge_candidates(vec!["192.168.0.1".parse().unwrap()], None, 24900);
        assert_eq!(only.len(), 1);
        // 発見なし → 指定のみ
        let fallback = merge_candidates(Vec::new(), Some("100.100.10.9"), 24900);
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].to_string(), "100.100.10.9:24900");
        // 両方なし → 空(呼び出し側は再試行へ落ちる)
        assert!(merge_candidates(Vec::new(), None, 24900).is_empty());
    }
```

- [ ] **Step 2: テストを実行して失敗を確認**

```bash
source $HOME/.cargo/env && cargo test -p tsunagu-common merge_candidates
```

期待: コンパイルエラー(`merge_candidates` が未定義)

- [ ] **Step 3: 実装**

(1) `crates/common/src/lib.rs` の discover モジュールから `seek`(565-585 行)と
`seek_lan`(587-590 行)を削除する(併合後の参照が無くなるため。Task 1 で追加した
`seek_first` / `seek_first_lan` だけが残る)。

(2) connect モジュールの `resolve`(617-625 行)を以下へ置き換え:

```rust
    /// 発見結果と手動指定を併合する(発見を先頭・IP+ポート単位で重複排除)。
    /// LAN 直と Tailscale を両方候補へ並べるため、first_reachable が自然に最速経路を採用する
    fn merge_candidates(found: Vec<std::net::IpAddr>, hosts: Option<&str>, port: u16) -> Vec<SocketAddr> {
        let mut addrs: Vec<SocketAddr> = found.into_iter().map(|ip| SocketAddr::new(ip, port)).collect();
        if let Some(h) = hosts {
            for a in parse_hosts(h, port) {
                if !addrs.contains(&a) {
                    addrs.push(a);
                }
            }
        }
        addrs
    }

    /// 接続候補: LAN 自動発見の結果を先頭に、指定(TSUNAGU_HOST)を併せて返す。
    /// 同じ LAN にいれば発見=LAN 直が最速で、いなければ指定(Tailscale 等)へフォールバックする
    pub fn resolve(hosts: Option<&str>, port: u16, token: &str) -> Vec<SocketAddr> {
        let found = crate::discover::seek_first_lan(port, token).into_iter().collect::<Vec<_>>();
        merge_candidates(found, hosts, port)
    }
```

- [ ] **Step 4: テストを実行して通過を確認**

```bash
cargo test -p tsunagu-common
```

期待: 既存全テスト + 追加 2 件が PASS(実ネットワーク環境の LAN に応答者がいても、`resolve` 自体は候補を増やすだけなので既存テストに影響なし)

- [ ] **Step 5: win 側の候補ゼロ時ログを実際の挙動へ合わせる**

`crates/win/src/main.rs:1106`:

```rust
            println!("[conn] 接続先が見つかりません(TSUNAGU_HOST 未指定時は同じ LAN の Mac を探します)");
```

↓(指定の有無にかかわらず発見を試むようになったため)

```rust
            println!("[conn] 接続先が見つかりません(LAN の Mac を発見できず TSUNAGU_HOST の候補も空です)");
```

- [ ] **Step 6: Windows 向けビルドでコンパイル確認**

```bash
cargo build -p tsunagu-win --target x86_64-pc-windows-gnu
```

期待: エラー無し

- [ ] **Step 7: コミット**

```bash
git add crates/common/src/lib.rs crates/win/src/main.rs
git commit -m "$(cat <<'EOF'
改善ループN(第11セッション): 接続候補を LAN 発見 ∪ TSUNAGU_HOST の併合へ(発見優先)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Mac の待受を全インターフェースへ

**Files:**
- Modify: `scripts/restart-mac.sh:31-32`

**Interfaces:**
- Consumes: なし(スクリプト 1 行)
- Produces: Mac の本線 24900・音声 24901・bulk 24902 が全 IF で待受(`TSUNAGU_BIND` の上書き経路は残す)

- [ ] **Step 1: BIND_IP の既定を変更**

`scripts/restart-mac.sh`:

```bash
BIND_IP=$(tailscale ip -4 2>/dev/null | head -1)
[ -z "$BIND_IP" ] && BIND_IP="0.0.0.0"
```

↓

```bash
# Tailscale IP へ束ねると LAN からの直接が届かないため、全 IF で待受ける。
# 接続元の防御は is_allowed(LAN/直結/Tailscale 絞り)+ Noise ハンドシェイクが担う
BIND_IP="0.0.0.0"
```

(`TSUNAGU_BIND=$BIND_IP …` の行は変更しない。環境変数 TSUNAGU_BIND を先にexport すれば上書き可、の経路は温存)

- [ ] **Step 2: 構文確認**

```bash
bash -n scripts/restart-mac.sh
```

期待: エラー無し

- [ ] **Step 3: コミット**

```bash
git add scripts/restart-mac.sh
git commit -m "$(cat <<'EOF'
改善ループN(第11セッション): Mac の待受を 0.0.0.0 へ(LAN 直を受け入れる)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: ドキュメント更新(usage.md / CHANGELOG.md)

**Files:**
- Modify: `docs/usage.md:67-68`(LAN 自動発見の説明)、`docs/usage.md:161` 付近(設定表)
- Modify: `CHANGELOG.md`(Unreleased セクションを新設)

**Interfaces:**
- Consumes: Task 1-3 の実装結果(挙動の説明文)
- Produces: なし(文書)

- [ ] **Step 1: usage.md の接続セクションを併合動作へ書き換え**

`docs/usage.md:67-68`:

```markdown
`TSUNAGU_HOST` を省略すると、同じ LAN にいる Mac を自動で探します(UDP 24903。
AP 隔離や Tailscale 越しではブロードキャストが届かないため、その場合は指定が必要)。
```

↓

```markdown
接続先は **LAN 自動発見(UDP 24903)と `TSUNAGU_HOST` の併用**です。起動のたびにまず同じ
LAN の Mac を探し(最初の応答 or 600ms)、見つかった LAN IP を先頭に `TSUNAGU_HOST` の
候補を並べて同時接続レースへかけます。つまり **同じ LAN では Tailscale の状態に
関係なく常時 LAN 直**、LAN 外(AP 隔離・外出先)では `TSUNAGU_HOST` の Tailscale IP へ
自動フォールバックします。
```

- [ ] **Step 2: usage.md の設定表へ TSUNAGU_BIND を追加**

`docs/usage.md` の設定表(160-161 行)で `TSUNAGU_HOST` 行の前に 1 行追加し、`TSUNAGU_HOST` 行の説明を更新:

```markdown
| `TSUNAGU_BIND` | 0.0.0.0 | Mac 側の待受アドレス。既定は全インターフェース(LAN 直を受け入れる) |
| `TSUNAGU_HOST` | (未設定) | Windows 側の接続先(フォールバック候補)。LAN 自動発見とは併用で、見つかった LAN IP が優先される |
```

- [ ] **Step 3: CHANGELOG.md へ Unreleased セクションを追加**

先頭の `## [0.23.0] - 2026-09-26` の前に挿入:

```markdown
## [Unreleased]

### 通信
- 接続経路を LAN 直優先へ: 同じ LAN では Tailscale の状態と無関係に LAN 直で接続し、
  LAN 外では `TSUNAGU_HOST`(Tailscale 等)へ自動フォールバック。接続候補を「LAN 自動発見 ∪
  TSUNAGU_HOST」の併合(発見優先・重複排除)へ変え、発見に最初の応答 or 600ms で返す
  `discover::seek_first` を追加。Mac の待受を全インターフェース(0.0.0.0)へ拡大。
  **プロトコル変更なし(版 11 のまま。片側だけの配備でも既存接続は壊れない)**
```

- [ ] **Step 4: コミット**

```bash
git add docs/usage.md CHANGELOG.md
git commit -m "$(cat <<'EOF'
改善ループN(第11セッション): LAN 直優先化に合わせて usage/CHANGELOG を更新

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: 全体ビルド・全テスト・実機検証の依頼

- [ ] **Step 1: 全クレートのテストとビルド**

```bash
source $HOME/.cargo/env
cargo test -p tsunagu-common
cargo build -p tsunagu-mac --release
cargo build -p tsunagu-win --release --target x86_64-pc-windows-gnu
```

期待: 全てエラー無し(コミット済みの状態と同じ結果であることを最終確認)

- [ ] **Step 2: 実機検証の依頼(ユーザー実行。結果を貼ってもらう)**

ユーザーへ以下を提示して待つ:

```bash
./scripts/restart-mac.sh --diag
./scripts/deploy-win.sh
./scripts/verify.sh
```

確認点(結果を貼ってもらったら確認する):
- Windows ログ: `ssh home "type C:\Users\<user>\tsunagu\tsunagu-win.log"` が
  `[conn] connected (192.168.0.1:24900)`(LAN IP)になっていること
- `[bulk] established (→192.168.0.1:24902)`・音声も同じ LAN IP であること
- verify.sh 全項目 pass(目安 pass=17 fail=0)
- Mac 側 `tailscale status` の経路表示との照合(参照情報)

- [ ] **Step 3: 結果の記録**

実機結果をCHANGELOG の Unreleased へ(必要なら)追記し、経過を docs/improvement-log.md の
運用に合わせて記録する。
