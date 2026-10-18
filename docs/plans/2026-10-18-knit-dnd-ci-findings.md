# CI(push)を有効にして分かったこと — D&D改善の副産物

対象: 2026-10-18 の D&D 改善作業で `main` を push し、GitHub Actions(`ci.yml`)を
回した結果。**macOS と windows-latest の両方が緑になったのは 2026-10-05 以来**。
実行: run [39082163530](https://github.com/reoanrudo/knit/actions/runs/39082163530)
(macos ✓ 3m22s / windows ✓ 1m20s、「回帰なし」)。

## なぜ今まで気づかなかったか

`ci.yml` の windows ジョブは `cargo test -p knit-common -p knit-win` を回すが、
**この数週間ずっと赤だった**(run 37298442641 = 2026-10-05 も failure)。
cargo test は最初に落ちたテストバイナリで止まるため、**後ろのテストは一度も
実行されていなかった**。ローカルの macOS では `knit-win` をコンパイルできないため、
Windows 固有のコードとテストは「書いたが検証されていない」状態だった。

## 見つかった4件(重要度順)

### 1. [製品バグ・重要] 掴みドラッグが「一度クリックすると以後ずっと保存のみ」になる

- 検出: `knit-win` の `dragdrop::tests::transfer_from_mac_starts_a_drag_only_while_the_carried_press_continues`
  (`left: Released, right: Carried`)
- 原因: `knit_common::drag::Carried::release()` が立てる `released_once` を**誰も消さなかった**。
  Windows 側は**左ボタンを離すたび**に `relay_up()` → `release()` を呼ぶため、
  モード切替後に一度クリックしただけで、以後の Mac→Win の掴みドラッグが
  すべて `Claim::Released`(= 相手は OLE を始めず、受信フォルダに保存のみ)と判定される。
- 影響: ユーザーが報告していた「**置いたのに置かれていない**」(摩擦 F5)の一因。
  設計の前提「押下が続いている間に届いた転送だけをドラッグにする」が、
  実機では最初のクリック以降ずっと成立していなかった。
- 修正: `Carried::press()`(押下の開始)と `claim()` の消費で記録を消す。
  Windows 側は左ボタン押下で `dragdrop::note_press()` を呼ぶ。
  遅着した予告を「離れた後」として扱う保護は維持。
  テスト `a_new_drag_after_a_release_is_carried_again` で両方の経路を固定。
- コミット: `c49f95d`

### 2. [回帰・私が入れた] Windows で受信ファイルの更新日時が保存されない

- 検出: `bulk_receive::receiver_restores_declared_mtime_through_the_open_handle`
  (宣言 1700000000 に対し実測 現在時刻)
- 原因: 性能改善ループ2(`29f65e2`)で「閉じる前の fd へ `set_modified`」に変えたが、
  Windows では更新日時が動く箇所が**2つ**あった: (a) 書き込みハンドルを閉じる時点、
  (b) `mark_untrusted` が書く `Zone.Identifier`(ADS)。
  macOS/Linux では閉じても変わらないため、ローカルでは気づけなかった。
- 修正: Windows は**公開(publish)が全部終わった後**に最終パスへ設定。
  macOS/Linux は従来どおり fd へ設定(ループ2の計測は活かす)。
- コミット: `1047f61`(第1版)→ `ecf68a7`(ADS の分を直して完成)

### 3. [既存] Windows CI が赤だった直接の原因(テストの区切り文字)

- 検出: `bulk_receive::folder_drag_roundtrips_with_structure_content_and_mtime`
  (期待 `資料フォルダ/a.txt`、実測 `資料フォルダ\a.txt`)
- 原因: テストが `/` 区切りの相対パスを期待していた(2026-10-05 の run でも同じ失敗)。
- 修正: 構造の比較なので `/` へ正規化。コミット `1047f61`

### 4. [入力検証] base64 の過剰パディングを受け付けていた

- 検出: `tray::preferences::tests::base64_rejects_malformed_input`
  (`b64_decode("AAAA====")` が `Some` を返していた)
- 原因: 末尾の `=` を**無制限に**捨てていた。
- 修正: パディングは末尾のみ・最大2つに制限。コミット `c49f95d`

### 補足: テストインフラの脆弱性

`pairing` のテストが Windows ランナーで `WSAEACCES(10013)` により落ちた
(解放直後のポートへの再 bind が Hyper-V/WinNAT の予約範囲に当たる)。
ポート確保と bind を最大15回試すようにした(テスト専用)。コミット `2fbf21d`。

## 学び(今後に効かせること)

1. **`--exclude knit-win` で隠れていた範囲は CI でしか守れない。** ローカルの緑は
   Windows の緑を意味しない。push を検証ループに入れて初めて、上の1〜4が出た。
2. **cargo test は最初の失敗で止まる。** 赤を放置すると、その後ろのテストは
   「存在するが一度も走っていない」状態になる。赤は早く消す。
3. **性能最適化は出口(公開後)まで含めて考える。** 「閉じる前に設定すれば速い」は
   Windows では成立しない(閉じる時点と ADS 書き込みで動く)。
