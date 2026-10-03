use std::sync::mpsc::Receiver;

pub(crate) struct Queued {
    pub generation: u64,
    pub line: String,
}

impl Queued {
    pub(crate) fn for_generation(self, generation: u64) -> Option<String> {
        (self.generation == generation).then_some(self.line)
    }
}

pub(crate) fn batch(
    mut first: Queued,
    receiver: &Receiver<Queued>,
    pending: &mut Option<Queued>,
) -> Queued {
    let moves = first.line.starts_with("{\"t\":\"mouse_abs\"");
    for _ in 0..32 {
        if first.line.len() > 256 * 1024 {
            break;
        }
        let Ok(next) = receiver.try_recv() else { break };
        if next.generation != first.generation {
            *pending = Some(next);
            break;
        }
        if moves && next.line.starts_with("{\"t\":\"mouse_abs\"") {
            first.line = next.line;
        } else {
            first.line.push_str(&next.line);
            if moves {
                break;
            }
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;
    fn queued(generation: u64, line: &str) -> Queued {
        Queued {
            generation,
            line: line.into(),
        }
    }

    #[test]
    fn batching_does_not_send_previous_device_input_to_the_next_device() {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(queued(2, "new-device-key\n")).unwrap();
        let mut pending = None;
        let old = batch(queued(1, "old-device-key\n"), &receiver, &mut pending);
        assert_eq!(old.line, "old-device-key\n");
        let next = pending.take().expect("次の接続の入力を失わない");
        assert_eq!(next.generation, 2);
        assert_eq!(next.line, "new-device-key\n");
    }

    #[test]
    fn mouse_coalescing_keeps_key_order_and_the_next_session() {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(queued(1, "{\"t\":\"mouse_abs\",\"x\":2}\n"))
            .unwrap();
        sender.send(queued(1, "key-down\n")).unwrap();
        sender.send(queued(2, "next-device\n")).unwrap();
        let mut pending = None;
        let result = batch(
            queued(1, "{\"t\":\"mouse_abs\",\"x\":1}\n"),
            &receiver,
            &mut pending,
        );
        assert_eq!(result.line, "{\"t\":\"mouse_abs\",\"x\":2}\nkey-down\n");
        assert_eq!(receiver.try_recv().unwrap().generation, 2);
    }

    #[test]
    fn stale_input_is_discarded_at_the_writer() {
        assert!(queued(1, "old-key\n").for_generation(2).is_none());
        assert_eq!(
            queued(2, "new-key\n").for_generation(2).as_deref(),
            Some("new-key\n")
        );
    }
}
