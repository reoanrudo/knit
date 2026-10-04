use crate::PeerEntry;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Departure {
    Unchanged,
    Active { next: Option<String> },
}

pub(crate) fn insert(peers: &mut Vec<PeerEntry>, active: usize, peer: PeerEntry) -> bool {
    let select = active >= peers.len() || peers.get(active).is_some_and(|p| p.id == peer.id);
    if let Some(i) = peers.iter().position(|p| p.id == peer.id) {
        let mut peer = peer;
        peer.side = peers[i].side;
        peer.edge_monitor = peers[i].edge_monitor;
        // エイリアス(表示名)も配置と同じく既存エントリの値を優先して引き継ぐ。
        // 呼び出し側が peer-sides.json から積み直すため通常は同じ値だが、保存の
        // 一時的な読み失敗で利用者が付けた名前が再接続で消えるのを防ぐ保険
        peer.alias = peers[i].alias.clone();
        if let Some(old) = peers[i].writer.take() {
            old.shutdown();
        }
        peers[i] = peer;
    } else {
        peers.push(peer);
    }
    select
}

pub(crate) fn detach(
    peers: &mut Vec<PeerEntry>,
    active: &mut usize,
    id: &str,
    generation: u64,
) -> Departure {
    let Some(i) = peers.iter().position(|p| p.id == id && p.gen == generation) else {
        return Departure::Unchanged;
    };
    let was_active = *active == i;
    peers.remove(i);
    let next = if was_active && !peers.is_empty() {
        Some(peers[i.min(peers.len() - 1)].id.clone())
    } else {
        None
    };
    if was_active {
        *active = usize::MAX;
    } else if *active != usize::MAX && *active > i {
        *active -= 1;
    }
    if was_active {
        Departure::Active { next }
    } else {
        Departure::Unchanged
    }
}

/// detach の後で端末の配置(peer-sides)を保存し直すべきか(純関数)。
/// 全端末がいなくなった時だけ: 1 台の切断のたびに保存すると、一時的な
/// ネット断の detach で「戻ってくるはずの端末の設定」を消してしまう。
/// 一方で detach が一切保存へ反映されないと、登録解除した端末の設定が
/// dead entry として残り続ける。全端末いなくなった時点=運用の区切りだけ
/// 保存すれば、次に現れた端末から整った一覧で始まる
pub(crate) fn should_save_sides_after_detach(peers_after: &[PeerEntry], departure: &Departure) -> bool {
    peers_after.is_empty() && !matches!(departure, Departure::Unchanged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: &str, generation: u64) -> PeerEntry {
        PeerEntry {
            id: id.into(),
            name: id.into(),
            ip: "127.0.0.1".parse().unwrap(),
            screen: (1920.0, 1080.0),
            monitors: Vec::new(),
            writer: None,
            gen: generation,
            side: 0,
            ver: knit_common::proto::VERSION,
            edge_monitor: None,
            alias: None,
        }
    }

    #[test]
    fn background_disconnect_keeps_the_selected_peer() {
        let mut peers = vec![peer("idle", 1), peer("working", 2)];
        let mut active = 1;
        assert_eq!(
            detach(&mut peers, &mut active, "idle", 1),
            Departure::Unchanged
        );
        assert_eq!(active, 0);
        assert_eq!(peers[active].id, "working");
    }

    #[test]
    fn active_disconnect_invalidates_the_old_writer_owner_before_fallback() {
        let mut peers = vec![peer("working", 1), peer("idle", 2)];
        let mut active = 0;
        assert_eq!(
            detach(&mut peers, &mut active, "working", 1),
            Departure::Active {
                next: Some("idle".into())
            }
        );
        assert_eq!(active, usize::MAX);
    }

    #[test]
    fn a_new_device_does_not_steal_control_from_the_selected_device() {
        let mut peers = vec![peer("working", 1)];
        assert!(!insert(&mut peers, 0, peer("new", 2)));
        assert_eq!(peers[0].id, "working");
        assert!(insert(&mut peers, 0, peer("working", 3)));
    }

    #[test]
    fn inactive_reconnection_keeps_control_and_its_saved_layout() {
        let mut peers = vec![peer("working", 1), peer("idle", 2)];
        peers[1].side = 2;
        peers[1].edge_monitor = Some(1);
        assert!(!insert(&mut peers, 0, peer("idle", 3)));
        assert_eq!(peers[1].side, 2);
        assert_eq!(peers[1].edge_monitor, Some(1));
        let mut active = 0;
        assert_eq!(
            detach(&mut peers, &mut active, "idle", 2),
            Departure::Unchanged
        );
        assert_eq!(peers.len(), 2);
    }

    /// 再接続(同じ id への insert 置換)では、利用者が付けたエイリアス(表示名)も
    /// 配置(side/edge_monitor)と同じく既存エントリから引き継がれる。
    /// 新しいエントリ側が alias 無しで積まれても名前が消えない(保存の読み失敗保険)
    #[test]
    fn reconnection_keeps_the_user_given_alias() {
        let mut peers = vec![peer("working", 1)];
        peers[0].alias = Some("事務室のPC".into());
        // 選択中の端末の再接続のため insert は選択を返す(既存テストと同じ意味)
        assert!(insert(&mut peers, 0, peer("working", 2)));
        assert_eq!(peers[0].alias.as_deref(), Some("事務室のPC"));
    }

    #[test]
    fn the_first_device_is_selected_and_the_last_disconnect_has_no_fallback() {
        let mut peers = Vec::new();
        assert!(insert(&mut peers, usize::MAX, peer("first", 1)));
        let mut active = 0;
        assert_eq!(
            detach(&mut peers, &mut active, "first", 1),
            Departure::Active { next: None }
        );
        assert_eq!(active, usize::MAX);
        assert!(peers.is_empty());
    }

    /// detach 後の配置保存は「全端末がいなくなった時」だけ。1 台残りの切断で
    /// 保存すると一時的なネット断で戻る端末の設定が消えるため
    #[test]
    fn sides_are_saved_only_when_the_last_peer_departed() {
        // 最後の 1 台が detach(対象がいた)→ PEERS 空 → 保存
        let mut peers = vec![peer("last", 1)];
        let mut active = 0;
        let departure = detach(&mut peers, &mut active, "last", 1);
        assert!(should_save_sides_after_detach(&peers, &departure));

        // 2 台のうち 1 台が detach → 残り 1 台 → 保存しない
        let mut peers = vec![peer("a", 1), peer("b", 2)];
        let mut active = 1;
        let departure = detach(&mut peers, &mut active, "a", 1);
        assert!(
            !should_save_sides_after_detach(&peers, &departure),
            "残り 1 台いるのに保存してはいけない"
        );

        // detach 対象が一覧に無い(Unchanged)→ PEERS 空でも保存しない
        let peers = Vec::new();
        assert!(!should_save_sides_after_detach(
            &peers,
            &Departure::Unchanged
        ));
    }
}
