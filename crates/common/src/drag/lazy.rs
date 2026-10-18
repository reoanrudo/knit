//! 遅延提供(drag.lazy)の状態機械(I8)。**ドロップを先に受理し、転送は後から流す**。
//!
//! なぜ必要か: 実測で Wi-Fi 実効 30MB/s のとき 1GB=34秒・10GB=5分41秒。現行方式は
//! 「全ファイルの到着後に相手が OLE ドラッグを開始する」ため、その間ユーザーは
//! 物理ボタンを押し続けることになり、大容量で成立しない。遅延提供では
//! 「目録 → 受理 → 流す」の順にして、**押下を転送量の関数にしない**。
//!
//! 設計方針: 判断はすべてこの純関数に置き、各プラットフォームは `LazyCommand` を
//! 実行するだけにする(macOS で全遷移をテストできる = Windows 側に判断を書かない)。
use super::Refusal;

/// 状態。`Idle` から `Dropped`/`Aborted` へ向かう一方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LazyState {
    /// 何も始まっていない(遅延提供を使わない相手・通常の転送)
    Idle,
    /// 掴みが越境した(目録を作って送る段階)
    Offered,
    /// 目録を送った(相手の受理待ち)
    ManifestSent,
    /// 相手が受理した(**この時点でドロップは成立している**。データは後から流す)
    Accepted,
    /// データを流している
    Streaming,
    /// 相手が置いた(正常終端)
    Dropped,
    /// 途中で終わった(異常終端)
    Aborted,
}

/// 異常終端の理由(利用者に見せるときは `Refusal` の文言と同じ扱いにする)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LazyAbort {
    /// 受理の前に離した(遅延提供では救えない。現行方式=保存のみへ落ちる)
    Released,
    /// Esc・メニューで中止した
    Escape,
    /// 接続が切れた
    Disconnected,
    /// 相手が時間内に受理しなかった
    TimedOut,
    /// 相手が断った(容量・件数など)
    PeerRefused { reason: Refusal },
    /// 目録の件数と実際に届いた件数が一致しない
    ManifestMismatch { expected: usize, received: usize },
}

/// 状態機械が出す命令。**実行するのは各プラットフォーム**(判断はここにしか無い)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LazyCommand {
    /// 目録(Manifest)を送れ。**受理を待たずに相手側のドラッグを始めてよい**
    SendManifest,
    /// 相手側でネイティブドラッグ(OLE / NSDragging)を開始せよ
    StartNativeDrag,
    /// データを流せ(受理後)
    Stream,
    /// 一時ファイルを全部捨てよ(**終端で一度だけ**)
    DiscardTemporary,
    /// CANCEL を送れ(相手の待ち状態を解く)
    SendCancel,
    /// 利用者に知らせよ
    Notify(LazyAbort),
}

/// 目録を送る側か受け取る側か。片付けの対象が変わる
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LazySide {
    /// 送る側(Mac→Win): 一時ファイルは持たない(元ファイルは残す)
    Sender,
    /// 受け取る側(Win→Mac): 中止したら受信途中の一時ファイルを捨てる
    Receiver,
}

/// 遅延提供の状態機械
#[derive(Debug, Clone)]
pub struct LazyHandoff {
    state: LazyState,
    side: LazySide,
    /// 目録で予告した件数(受信完了時に照合する)
    expected: usize,
}

impl Default for LazyHandoff {
    fn default() -> Self {
        Self::new(LazySide::Sender)
    }
}

impl LazyHandoff {
    pub const fn new(side: LazySide) -> Self {
        Self {
            state: LazyState::Idle,
            side,
            expected: 0,
        }
    }

    pub const fn state(&self) -> LazyState {
        self.state
    }

    /// 掴みが越境した。**遅延提供に対応した相手にだけ**目録を送る。
    /// 対応していなければ何も出さず `Idle` のまま(=現行方式へ自動フォールバック)
    pub fn offer(&mut self, peer_lazy: bool, count: usize) -> Vec<LazyCommand> {
        if !peer_lazy || self.state != LazyState::Idle || count == 0 {
            return Vec::new();
        }
        self.state = LazyState::Offered;
        self.expected = count;
        vec![LazyCommand::SendManifest]
    }

    /// 目録を送り終えた(相手側のドラッグはここから始められる)
    pub fn manifest_sent(&mut self) -> Vec<LazyCommand> {
        if self.state != LazyState::Offered {
            return Vec::new();
        }
        self.state = LazyState::ManifestSent;
        vec![LazyCommand::StartNativeDrag]
    }

    /// 相手が受理した(ドロップは先に成立。ここからデータを流す)
    pub fn accepted(&mut self) -> Vec<LazyCommand> {
        if !matches!(self.state, LazyState::ManifestSent | LazyState::Offered) {
            return Vec::new();
        }
        self.state = LazyState::Streaming;
        vec![LazyCommand::Stream]
    }

    /// 押下が離れた。**受理後なら正常**(相手がその場で置く)。受理前は救えない
    pub fn released(&mut self) -> Vec<LazyCommand> {
        match self.state {
            LazyState::Accepted | LazyState::Streaming => Vec::new(),
            LazyState::Offered | LazyState::ManifestSent => self.abort(LazyAbort::Released),
            _ => Vec::new(),
        }
    }

    /// データを流し終えた。**件数が一致したときだけ**ドロップ成立にする
    pub fn completed(&mut self, received: usize) -> Vec<LazyCommand> {
        if !matches!(self.state, LazyState::Accepted | LazyState::Streaming) {
            return Vec::new();
        }
        if received != self.expected {
            return self.abort(LazyAbort::ManifestMismatch {
                expected: self.expected,
                received,
            });
        }
        self.state = LazyState::Dropped;
        Vec::new()
    }

    /// 異常終端。**片付けの命令は一度だけ**出る。中止・切断・時間切れのどれでも同じ終端で、
    /// 終端に着いたら状態が `Aborted` になるため二度目は何も出ない(状態そのものが番人。
    /// 別に「出したか」のフラグを持つと状態ガードと重複して dead state になる: 変異テストで確認)
    pub fn abort(&mut self, why: LazyAbort) -> Vec<LazyCommand> {
        if matches!(self.state, LazyState::Idle | LazyState::Dropped | LazyState::Aborted) {
            return Vec::new();
        }
        self.state = LazyState::Aborted;
        let mut out = Vec::new();
        {
            // 目録を送った後なら相手が待っている。待ち状態を必ず解く
            out.push(LazyCommand::SendCancel);
            if self.side == LazySide::Receiver {
                out.push(LazyCommand::DiscardTemporary);
            }
        }
        out.push(LazyCommand::Notify(why));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_delivery_does_not_wait_for_the_bytes() {
        // 目録 → 受理 → 流す。**受理は「全データ到着」を待たない**のが要点
        let mut h = LazyHandoff::new(LazySide::Sender);
        assert_eq!(h.offer(true, 3), vec![LazyCommand::SendManifest]);
        assert_eq!(h.manifest_sent(), vec![LazyCommand::StartNativeDrag]);
        assert_eq!(
            h.state(),
            LazyState::ManifestSent,
            "ここではまだ 1 バイトも流れていない"
        );
        assert_eq!(h.accepted(), vec![LazyCommand::Stream]);
        assert_eq!(h.state(), LazyState::Streaming);
        // 押下は受理の後に離れる(遅延提供ではこれが正常)
        assert!(h.released().is_empty(), "受理後の離しは中止ではない");
        assert!(h.completed(3).is_empty(), "件数一致でドロップ成立");
        assert_eq!(h.state(), LazyState::Dropped);
    }

    #[test]
    fn a_peer_without_lazy_falls_back_to_the_current_path() {
        // 版 16 以前の相手には目録を送らない(受理の行を知らないため)
        let mut h = LazyHandoff::new(LazySide::Sender);
        assert!(h.offer(false, 3).is_empty());
        assert_eq!(h.state(), LazyState::Idle, "現行方式のまま");
        // 件数 0 や二重開始も受け付けない
        assert!(h.offer(true, 0).is_empty());
        assert_eq!(h.offer(true, 1), vec![LazyCommand::SendManifest]);
        assert!(h.offer(true, 1).is_empty(), "二重開始しない");
    }

    #[test]
    fn releasing_before_acceptance_aborts_and_cleans_up_once() {
        let mut h = LazyHandoff::new(LazySide::Sender);
        h.offer(true, 2);
        h.manifest_sent();
        let out = h.released();
        assert_eq!(
            out,
            vec![
                LazyCommand::SendCancel,
                LazyCommand::Notify(LazyAbort::Released)
            ],
            "受理前に離したら相手の待ちを解く"
        );
        assert_eq!(h.state(), LazyState::Aborted);
        // 二度目の中断では片付けも通知も繰り返さない(最初の理由が勝つ)
        assert!(
            h.abort(LazyAbort::Disconnected).is_empty(),
            "終端は一度きり"
        );
        assert_eq!(h.state(), LazyState::Aborted);
    }

    #[test]
    fn a_receiver_discards_its_temporary_files_on_abort() {
        let mut h = LazyHandoff::new(LazySide::Receiver);
        h.offer(true, 1);
        h.manifest_sent();
        h.accepted();
        let out = h.abort(LazyAbort::Escape);
        assert!(out.contains(&LazyCommand::DiscardTemporary), "{out:?}");
        assert!(out.contains(&LazyCommand::SendCancel), "{out:?}");
    }

    #[test]
    fn a_count_mismatch_never_becomes_a_drop() {
        // 既存の整合性方針(予告と実受信の件数一致)を遅延提供でも守る
        let mut h = LazyHandoff::new(LazySide::Receiver);
        h.offer(true, 5);
        h.manifest_sent();
        h.accepted();
        let out = h.completed(4);
        assert_eq!(
            out,
            vec![
                LazyCommand::SendCancel,
                LazyCommand::DiscardTemporary,
                LazyCommand::Notify(LazyAbort::ManifestMismatch {
                    expected: 5,
                    received: 4
                })
            ]
        );
        assert_eq!(h.state(), LazyState::Aborted, "欠けたまま置かせない");
    }

    #[test]
    fn every_abort_reason_reaches_the_same_terminal() {
        // 中止・切断・時間切れ・拒否のどれでも同じ終端(調査 M5)
        for why in [
            LazyAbort::Escape,
            LazyAbort::Disconnected,
            LazyAbort::TimedOut,
            LazyAbort::PeerRefused {
                reason: Refusal::Busy,
            },
        ] {
            let mut h = LazyHandoff::new(LazySide::Sender);
            h.offer(true, 1);
            h.manifest_sent();
            h.accepted();
            let out = h.abort(why.clone());
            assert_eq!(h.state(), LazyState::Aborted, "{why:?}");
            assert!(out.contains(&LazyCommand::SendCancel), "{why:?}: {out:?}");
            assert_eq!(out.last(), Some(&LazyCommand::Notify(why.clone())), "{why:?}");
        }
    }

    #[test]
    fn invalid_transitions_are_ignored() {
        let mut h = LazyHandoff::new(LazySide::Sender);
        // 何も始まっていないのに進めようとしても何も起きない
        assert!(h.manifest_sent().is_empty());
        assert!(h.accepted().is_empty());
        assert!(h.completed(1).is_empty());
        assert!(h.released().is_empty());
        assert!(h.abort(LazyAbort::Escape).is_empty(), "Idle では片付け不要");
        assert_eq!(h.state(), LazyState::Idle);
        // 受理を経ずに完了はできない
        h.offer(true, 1);
        h.manifest_sent();
        assert!(h.completed(1).is_empty(), "受理前の完了は無視");
        assert_eq!(h.state(), LazyState::ManifestSent);
        // ドロップ成立後の中断は無効(片付けを出さない)
        h.accepted();
        h.completed(1);
        assert!(h.abort(LazyAbort::Escape).is_empty(), "落ちた後の中断は無効");
        assert_eq!(h.state(), LazyState::Dropped);
    }
}
