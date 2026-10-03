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
}
