# 新機能 2 件(IME 引継ぎ・Continue Here)の実装直後レビュー

- 作成日: 2026-09-27(改善ループ 467/468 実装直後に未関与エージェント 2 視点で査読)
- 対象: 97dd5d6(IME Follow Cursor)、473b6d2(Continue Here)
- 結果: 指摘のうち実害あり 5 点を 477/478 で即時修正、残りは見送り理由付きで記録

---

## 修正に反映した指摘(477/478)

1. **⌥⌘T のキーリピート爆発**(edge-case最重要): 押下エッジ判定が無く、
   T を押しっぱなしにすると osascript 数十プロセス→Windows で同数のタブが
   開く。→ 押下エッジ(CONT_T_DOWN)+1.5 秒デッドタイムで発火を 1 回に制限。
   あわせて up も握る(down だけ握ると up 単体が Windows へ転送され、修飾の
   押し替えが前面アプリへ漏れる)
2. **前面ブラウザの固定順探査**: Safari が裏で常駐しているだけで、ユーザーが
   見ている Chrome ではなく Safari の古いページを渡す。→ System Events で
   frontmost プロセス名を特定してから、そのブラウザだけ読む
3. **AppleScript の try 無し**: DevTools/PWA 特殊ウィンドウで URL 取得が
   エラーになるとスクリプト全体が停止し以降のブラウザを試せない。→ 各 tell を
   try で囲みフォールスルー
4. **IME 自動同期のトグル反転**: IME ウィンドウが取れない窓(コンソール等)で
   VK_KANJI のトグルへ落ちると、切替のたびに IME が反転し続ける。→
   ime_set_open_impl(open, allow_toggle_fallback) に分離し、自動同期は
   スキップ+ログ、手動キーのみフォールバック維持(478)
5. **URL 検査の強化**: 受信側の文字列がそのまま ShellExecuteW へ渡るため、
   制御文字・空白の混入を urlx::transferable で拒否(テスト追加)

## 誤指摘として対応しなかったもの

- **TISGetInputSourceProperty の戻り値解放**: Apple の Get 系命名規則
  (Create/Copy のみ呼び出し側が解放)では解放不要。TISCopyCurrent... は
  現状どおり CFRelease 済み

## 記録のみ(実機確認後に再検討)

- TCC(自動化)未承認の初回に Windows 画面操作中は Mac のダイアログに気づけない
  → 失敗時の通知(60 秒デッドマン)で軽減済み。実機で初回許可の流れを確認
- ⌥⌘T が Windows 側の Ctrl+Alt+T 系ショートカットを奪う →
  TSUNAGU_CONTINUE_HERE=0 で無効化可能。usage.md に明記済み
- SendMessageW(ime_wnd) は相手 IME ウィンドウの応答を待つため、前面が
  ハングしたアプリだと受信ループの遅延になりうる → 実機で観測されたら
  PostMessageW/timeout 検討
- 467(97dd5d6)に BUILD_ID 破壊行が含まれたまま(bisect でこの 1 点のみ
  ビルド失敗。468 で自然修復)→ 罠 20 に記録。履歴修正はしない
