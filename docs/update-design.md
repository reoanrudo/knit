# アプリ自動更新の設計

作成: 2026-09-30。状態: **Mac・Windows・Android のすべてで実装済み。署名鍵の登録までは無効。実機での通し試験は Mac のみ(一時フォルダ)。**
ロードマップ [Q4](plans/2026-09-30-global-commercialization-roadmap.md) の「更新・失敗時の復帰」に当たる。

## 目的と原則

- 利用者が zip を取り直さず、通知に従って安全に新版へ移れる。
- 更新経路が乗っ取られても、**所有者の署名鍵がなければ任意のコードを実行させられない**。
  アプリのコード署名(Apple/Windows)だけでは更新情報の真正性は守れないため、独立した署名を持つ。
- 失敗しても旧版で使い続けられる。更新は「置き換えてから確認」ではなく「検証してから置き換え」。
- 何も無断で送らない。更新確認で送る情報は、版・OS・チャンネルのみ(入力内容・端末名・ファイル名は送らない)。

## 全体の流れ

```
起動時/6時間ごと → update.json と update.json.sig を取得(https)
  → verify_manifest   署名検証(検証前に中身を解釈しない)
  → select            チャンネル・版(現在より新しい)・https・OS を確認
  → 通知(自動適用しない。利用者が「更新」を選ぶ)
  → 成果物を取得 → verify_artifact  サイズ・SHA-256
  → 一時領域へ展開 → 適用(OS別) → 起動確認 → 失敗なら旧版へ戻す
```

検証コアは [crates/common/src/update.rs](../crates/common/src/update.rs)(実装済み・8 テスト)。

## 更新情報(update.json)

署名対象は配布された JSON のバイト列そのもの(正規化に依存しない)。分離署名 `update.json.sig` は
Ed25519 署名の小文字 16 進。

```json
{
  "schema_version": 1,
  "channel": "stable",
  "version": "0.27.0",
  "protocol": 13,
  "notes_url": "https://…",
  "artifacts": [
    {"platform": "macos-arm64", "url": "https://…/Knit-0.27.0.zip", "size": 1234567, "sha256": "…"},
    {"platform": "windows-x64", "url": "https://…/Knit-win-0.27.0.zip", "size": 1234567, "sha256": "…"}
  ]
}
```

- `channel` を署名対象に含め、beta の情報を stable に流用する攻撃を拒否する。
- `version` が現在版以下なら拒否する(ダウングレード攻撃・古い情報の再配信)。
- `protocol` は旧版の相手が残る場合の案内用。`Msg` の版交渉(`MIN_VERSION`)に合わせ、
  通信できなくなる更新は先に相手側の更新を案内する。

## 検証コアの拒否条件(テスト済み)

| 入力 | 結果 |
|---|---|
| 信頼する鍵が 0 件 | 全て拒否(鍵未設定のまま更新が有効にならない) |
| 改変された更新情報・別の鍵の署名・不正な署名文字列 | `BadSignature` |
| 64KiB 超の更新情報 | 解釈前に `TooLarge` |
| 現在と同じ/古い版、チャンネル違い、`http://` の URL | `NotNewer` / `WrongChannel` / `InsecureUrl` |
| 成果物のサイズ超過・不足、ハッシュ不一致 | `SizeMismatch` / `HashMismatch`(宣言サイズを超えて読まない) |
| 鍵の交代期(複数鍵) | いずれか 1 つで検証できれば有効 |

## 署名鍵の扱い(所有者が決めること)

- Ed25519 の鍵ペアを **オフラインで 1 回生成**し、秘密鍵は CI に置かない(置く場合は
  リリース専用の Secret とし、タグ保護と承認必須の環境に限る)。公開鍵を `TRUSTED_KEYS` へ登録して初めて
  更新が有効になる。
- 鍵の交代: 新旧 2 本を含む版を先に配り、全員が移った後で旧鍵を外す。紛失・漏えい時に
  署名済みの旧版を巻き戻されないよう、版の単調増加チェックを鍵に依存させない。
- 公開鍵の登録・秘密鍵の保管は所有者の作業。**この作業が済むまで更新機能は出荷しない。**

## OS ごとの適用

| | 方針 | 確認が必要な点 |
|---|---|---|
| Mac | 展開した新しい `Knit.app` を検証し、旧版を退避して入れ替え、LaunchAgent で再起動 | **署名の Team が同じなら**アクセシビリティ許可が維持される。アドホック署名では毎回失われるので、Developer ID 署名が更新機能の前提 |
| Windows | 常駐 exe は自分自身を置換できないため、新版の複製(`--apply-update`)が旧プロセスの終了を待って入れ替え、新版を起動して 10 秒生きていれば確定、そうでなければ戻す。更新中は `update.lock` で毎分の自動復帰タスクによる旧版の起動を止める。取得と展開は OS 標準の `curl.exe` / `tar.exe` | 未署名の間は SmartScreen 警告が出る。実機での通し試験が必要 |
| Android | アプリが APK を取得・検査し、OS のインストーラ(PackageInstaller)へ渡す。最後は利用者が OS の確認画面で承認する。署名者が違う APK は OS が拒否する | 初回は「この提供元のアプリを許可」が必要。**開発中のデバッグ署名の版から配布用の署名へ移る時は 1 度だけアンインストールが必要**(登録も再度必要) |

共通: 設定・鍵(Keychain/DPAPI)・登録情報は更新で触らない。設定の形式を変える版は、
移行と旧版への書き戻し不能を更新情報で明示する。

## 実装の状況

| 項目 | 状態 | 場所 |
|---|---|---|
| 検証コア(署名・版・URL・ハッシュ) | ✅ 単体テスト 10 件 | [update.rs](../crates/common/src/update.rs) |
| 鍵生成・更新情報の生成・署名ツール | ✅ 結合テスト | `cargo run -p knit-common --bin knit-sign` |
| リリース時の `update.json` 自動生成 | ✅ Secret `KNIT_UPDATE_SIGNING_KEY` があれば生成(未検証: 実タグでの実行) | [release.yml](../.github/workflows/release.yml) |
| Mac: 確認・取得・検証・入れ替え・旧版復帰・メニュー・6 時間ごとの自動確認 | ✅ | [updater.rs](../crates/mac/src/updater.rs) |
| Mac: 通し試験(署名 → 取得 → 入れ替え、改変の拒否) | ✅ `scripts/tests/update-e2e.sh` | 一時フォルダのみ使用 |
| Windows: 確認・取得・展開・入れ替え・旧版復帰・トレイメニュー・自動確認 | ✅ 入れ替え/復帰と配布物検査は共有部品のテスト済み。**Windows 実機では未実行**(ビルドの確認のみ) | [updater.rs](../crates/win/src/updater.rs) |
| Android: 確認・取得・検査・インストール依頼・画面 | ✅ 検証はブリッジのテスト済み、画面はエミュレータで確認。**更新の適用(インストール)は未実行** | [Updater.java](../android/app/src/main/java/app/knit/Updater.java) |
| Android の署名付き配布物の作成 | ✅ 使い捨て鍵で通し確認 | [release-android.sh](../scripts/release-android.sh) |
| 独立レビューと修正 | ✅ 指摘の大半を修正。未対応の項目と理由は [レビュー記録](reviews/2026-09-30-update-review.md) | |
| 実機試験(Developer ID 署名下の権限維持、電源断、旧新混在) | 未実施 | |

### Mac の動作

1. 起動 90 秒後と 6 時間ごとに、更新情報を静かに確認する(`KNIT_AUTO_UPDATE=0` で停止)。
   新しい版があれば通知し、メニューが「Knit X.Y.Z に更新…」になる。自動では適用しない。
2. 選ぶと成果物を取得し、サイズ・SHA-256・展開後の署名・識別子・版を検査する。
   現行版が正式署名なら、新版の署名者が同じであることも求める(違うとアクセシビリティ許可が失われるため)。
3. アプリを終了し、別プロセスが旧版を退避 → 新版を配置 → 起動 → 6 秒生きていれば旧版を削除。
   起動しなければ旧版へ戻して再起動する。
4. 端末から確認・更新するには `Knit.app/Contents/MacOS/Knit --update`。

### Android の配布

Android アプリの版はデスクトップと独立して進む(`versionName`)。更新情報は固定タグ `android-latest` の
Release に置く(`releases/latest` は別の Release を指すため)。

```bash
KNIT_ANDROID_KEYSTORE=… KNIT_ANDROID_STORE_PASSWORD=… KNIT_UPDATE_KEY=~/knit-update.key ./scripts/release-android.sh
```

署名済み APK と `update-android.json(.sig)` が `dist/android-release/` にできる。公開コマンドは実行後に表示される。
配布用の署名鍵(keystore)は所有者が作って保管する。**この鍵を失うと、既存の利用者へ更新を配れなくなる。**

### 有効にする手順(所有者)

```bash
cargo run -p knit-common --bin knit-sign -- keygen ~/knit-update.key   # 公開鍵を表示
```

1. 表示された公開鍵を [update-keys.txt](../crates/common/update-keys.txt) へ追記してコミットする
2. 秘密鍵(`~/knit-update.key` の中身)を GitHub の Secret `KNIT_UPDATE_SIGNING_KEY` へ登録し、原本は安全な場所に保管する
3. 公開鍵入りの版を配り、その次のタグから `update.json` が付く

## 既知の制限

- 署名済みの古い更新情報を再配信されると、現在版より新しい中間版へ誘導され得る(有効期限・最低版は未導入)
- Mac の入れ替え中(数秒)に電源が落ちると、アプリが欠ける可能性がある。復旧処理は未実装
- 起動確認は「6 秒間プロセスが生きている」まで。アクセシビリティ未許可のような機能面の失敗は検知しない

## 未決事項

- 更新情報の置き場は当面 GitHub Releases の最新版(`MANIFEST_URL`)。独自ドメインへ移す場合は、次の版で置き場を変えた版を先に配る
- 確認の頻度と、利用者が無効にできる設定の置き場
- 開発者向けのローカル更新(`scripts/update-mac.sh`)は、この仕組みとは別に残す
