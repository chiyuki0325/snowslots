use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub const REWIND_BYTES: usize = 20 * 1024 * 1024;

pub struct Rewind {
    budget: usize,
    cur: Option<Vec<u8>>,
    deltas: VecDeque<Vec<u8>>,
    bytes: usize,
}

impl Rewind {
    pub fn new(budget_bytes: usize) -> Self {
        Rewind {
            budget: budget_bytes,
            cur: None,
            deltas: VecDeque::new(),
            bytes: 0,
        }
    }

    pub fn push(&mut self, state: &[u8]) {
        self.push_owned(state.to_vec());
    }

    fn push_owned(&mut self, state: Vec<u8>) {
        let Some(mut prev) = self.cur.replace(state) else {
            return;
        };
        let state = self.cur.as_ref().unwrap();
        if prev.len() != state.len() {
            self.deltas.clear();
            self.bytes = 0;
            return;
        }
        for (previous, current) in prev.iter_mut().zip(state) {
            *previous ^= current;
        }
        let entry = lz4_flex::compress(&prev);
        self.bytes += entry.len();
        self.deltas.push_back(entry);
        while self.bytes > self.budget {
            match self.deltas.pop_front() {
                Some(oldest) => self.bytes -= oldest.len(),
                None => break,
            }
        }
    }

    pub fn pop(&mut self) -> Option<Vec<u8>> {
        let cur = self.cur.take()?;
        let Some(entry) = self.deltas.pop_back() else {
            return Some(cur);
        };
        self.bytes -= entry.len();
        let mut prev = vec![0u8; cur.len()];
        match lz4_flex::decompress_into(&entry, &mut prev) {
            Ok(_) => {
                for (p, c) in prev.iter_mut().zip(&cur) {
                    *p ^= c;
                }
                self.cur = Some(prev);
            }
            Err(e) => {
                eprintln!("slot: rewind: {e}");
                self.deltas.clear();
                self.bytes = 0;
            }
        }
        Some(cur)
    }

    pub fn bytes_used(&self) -> usize {
        self.bytes
    }

    pub fn depth(&self) -> usize {
        self.deltas.len() + usize::from(self.cur.is_some())
    }

    pub fn fill(&self) -> u8 {
        if self.budget == 0 {
            return 0;
        }
        (self.bytes * 100 / self.budget).min(100) as u8
    }
}

pub struct RewindThread {
    tx: std::sync::mpsc::SyncSender<Msg>,
    fill: std::sync::Arc<std::sync::atomic::AtomicU8>,
    pending: Arc<AtomicUsize>,
}

enum Msg {
    Push(Vec<u8>),
    Pop(std::sync::mpsc::SyncSender<Option<Vec<u8>>>),
}

impl RewindThread {
    pub fn spawn(budget_bytes: usize) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Msg>(4);
        let fill = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
        let published = fill.clone();
        let pending = Arc::new(AtomicUsize::new(0));
        let processing = pending.clone();
        std::thread::Builder::new()
            .name("slot-rewind".into())
            .spawn(move || {
                let mut rewind = Rewind::new(budget_bytes);
                while let Ok(msg) = rx.recv() {
                    match msg {
                        Msg::Push(state) => {
                            rewind.push_owned(state);
                            published.store(rewind.fill(), std::sync::atomic::Ordering::Relaxed);
                            processing.fetch_sub(1, Ordering::Release);
                        }
                        Msg::Pop(reply) => {
                            let out = rewind.pop();
                            published.store(rewind.fill(), std::sync::atomic::Ordering::Relaxed);
                            let _ = reply.send(out);
                        }
                    }
                }
            })
            .expect("rewind thread");
        RewindThread { tx, fill, pending }
    }

    pub fn push(&self, state: Vec<u8>) {
        self.pending.fetch_add(1, Ordering::AcqRel);
        if self.tx.send(Msg::Push(state)).is_err() {
            self.pending.fetch_sub(1, Ordering::Release);
        }
    }

    pub fn try_record(&self, capture: impl FnOnce() -> Option<Vec<u8>>) -> bool {
        if self
            .pending
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }
        if let Some(state) = capture() {
            if self.tx.try_send(Msg::Push(state)).is_ok() {
                return true;
            }
        }
        self.pending.fetch_sub(1, Ordering::Release);
        false
    }

    pub fn pop(&self) -> Option<Vec<u8>> {
        let (tx, rx) = std::sync::mpsc::sync_channel(0);
        if self.tx.send(Msg::Pop(tx)).is_err() {
            return None;
        }
        rx.recv().ok().flatten()
    }

    pub fn fill(&self) -> u8 {
        self.fill.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU8;
    use std::sync::mpsc;

    fn idle_thread() -> (RewindThread, mpsc::Receiver<Msg>) {
        let (tx, rx) = mpsc::sync_channel(4);
        (
            RewindThread {
                tx,
                fill: Arc::new(AtomicU8::new(0)),
                pending: Arc::new(AtomicUsize::new(0)),
            },
            rx,
        )
    }

    #[test]
    fn a_pending_snapshot_skips_capture_before_serializing() {
        let (rewind, rx) = idle_thread();
        assert!(rewind.try_record(|| Some(vec![1, 2, 3])));
        assert!(!rewind.try_record(|| panic!("captured while compression was pending")));
        assert!(matches!(rx.try_recv(), Ok(Msg::Push(state)) if state == [1, 2, 3]));
        assert!(!rewind.try_record(|| panic!("captured before compression finished")));
        rewind.pending.fetch_sub(1, Ordering::Release);
        assert!(rewind.try_record(|| Some(vec![4, 5, 6])));
    }

    #[test]
    fn a_failed_capture_releases_the_reservation() {
        let (rewind, _rx) = idle_thread();
        assert!(!rewind.try_record(|| None));
        assert!(rewind.try_record(|| Some(vec![1])));
    }

    #[test]
    fn a_disconnected_worker_releases_the_reservation() {
        let (rewind, rx) = idle_thread();
        drop(rx);
        assert!(!rewind.try_record(|| Some(vec![1])));
        assert_eq!(rewind.pending.load(Ordering::Acquire), 0);
    }

    #[test]
    fn a_full_queue_does_not_block_recording() {
        let (rewind, _rx) = idle_thread();
        for _ in 0..4 {
            let (reply, _response) = mpsc::sync_channel(0);
            rewind.tx.send(Msg::Pop(reply)).unwrap();
        }
        assert!(!rewind.try_record(|| Some(vec![1])));
        assert_eq!(rewind.pending.load(Ordering::Acquire), 0);
    }

    #[test]
    fn owned_snapshots_keep_their_allocation() {
        let mut rewind = Rewind::new(1024);
        let first = vec![1, 2, 3];
        let first_ptr = first.as_ptr();
        rewind.push_owned(first);
        assert_eq!(rewind.cur.as_ref().unwrap().as_ptr(), first_ptr);
        let second = vec![4, 5, 6];
        let second_ptr = second.as_ptr();
        rewind.push_owned(second);
        assert_eq!(rewind.cur.as_ref().unwrap().as_ptr(), second_ptr);
        assert_eq!(rewind.pop(), Some(vec![4, 5, 6]));
        assert_eq!(rewind.pop(), Some(vec![1, 2, 3]));
    }
}
