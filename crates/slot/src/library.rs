use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

use slot_store::{remove_library_cache, scan_report, write_library_cache, Cart};

enum Job {
    Save(u64, Vec<Cart>),
    Scan(u64),
    Remove,
}

pub struct LibraryRefresh {
    jobs: mpsc::Sender<Job>,
    results: mpsc::Receiver<(u64, Option<Vec<Cart>>)>,
    revision: Arc<AtomicU64>,
    initial: Option<Vec<Cart>>,
    presented: bool,
    due: bool,
    scanning: bool,
    ready: Option<Vec<Cart>>,
}

impl LibraryRefresh {
    pub fn new(root: PathBuf, initial: Option<Vec<Cart>>, needs_scan: bool) -> Option<Self> {
        let (jobs, requests) = mpsc::channel();
        let (completed, results) = mpsc::channel();
        let revision = Arc::new(AtomicU64::new(0));
        let current = revision.clone();
        std::thread::Builder::new()
            .name("slot-library".into())
            .spawn(move || {
                while let Ok(job) = requests.recv() {
                    match job {
                        Job::Remove => {
                            if let Err(error) = remove_library_cache(&root) {
                                eprintln!("slot: library cache: {error}");
                            }
                        }
                        Job::Save(version, carts) => {
                            save_current(&root, &current, version, &carts);
                        }
                        Job::Scan(version) => {
                            if current.load(Ordering::Acquire) != version {
                                let _ = completed.send((version, None));
                                continue;
                            }
                            let scanned = scan_report(&root);
                            crate::boot_time::mark("library scan complete");
                            let carts = if scanned.complete {
                                save_current(&root, &current, version, &scanned.carts);
                                Some(scanned.carts)
                            } else {
                                eprintln!(
                                    "slot: library: incomplete scan, keeping the current shelf"
                                );
                                let _ = remove_library_cache(&root);
                                None
                            };
                            if completed.send((version, carts)).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .map_err(|error| eprintln!("slot: library thread: {error}"))
            .ok()?;
        Some(Self {
            jobs,
            results,
            revision,
            initial,
            presented: false,
            due: needs_scan,
            scanning: false,
            ready: None,
        })
    }

    pub fn presented(&mut self) {
        if self.presented {
            return;
        }
        self.presented = true;
        if let Some(carts) = self.initial.take() {
            let _ = self.jobs.send(Job::Save(self.version(), carts));
        }
    }

    pub fn invalidate(&mut self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        self.initial = None;
        self.ready = None;
        self.due = true;
        let _ = self.jobs.send(Job::Remove);
    }

    pub fn pending(&self) -> bool {
        self.initial.is_some() || self.due || self.scanning || self.ready.is_some()
    }

    pub fn poll(&mut self, allowed: bool) -> Option<Vec<Cart>> {
        while let Ok((version, carts)) = self.results.try_recv() {
            self.scanning = false;
            if version == self.version() {
                self.ready = carts;
            }
        }
        if !self.presented || !allowed {
            return None;
        }
        if self.due && !self.scanning {
            self.due = false;
            self.scanning = self.jobs.send(Job::Scan(self.version())).is_ok();
        }
        self.ready.take()
    }

    fn version(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
}

fn save_current(root: &std::path::Path, revision: &AtomicU64, version: u64, carts: &[Cart]) {
    if revision.load(Ordering::Acquire) != version {
        return;
    }
    if let Err(error) = write_library_cache(root, carts) {
        eprintln!("slot: library cache: {error}");
        let _ = remove_library_cache(root);
    }
    if revision.load(Ordering::Acquire) != version {
        let _ = remove_library_cache(root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controlled(
        needs_scan: bool,
    ) -> (
        LibraryRefresh,
        mpsc::Receiver<Job>,
        mpsc::Sender<(u64, Option<Vec<Cart>>)>,
    ) {
        let (jobs, requests) = mpsc::channel();
        let (completed, results) = mpsc::channel();
        (
            LibraryRefresh {
                jobs,
                results,
                revision: Arc::new(AtomicU64::new(0)),
                initial: None,
                presented: false,
                due: needs_scan,
                scanning: false,
                ready: None,
            },
            requests,
            completed,
        )
    }

    #[test]
    fn scanning_waits_for_a_presented_frame_and_a_safe_screen() {
        let (mut library, jobs, _completed) = controlled(true);
        assert!(library.poll(true).is_none());
        assert!(jobs.try_recv().is_err());
        library.presented();
        assert!(library.poll(false).is_none());
        assert!(jobs.try_recv().is_err());
        assert!(library.poll(true).is_none());
        assert!(matches!(jobs.try_recv(), Ok(Job::Scan(0))));
        library.poll(true);
        assert!(jobs.try_recv().is_err());
    }

    #[test]
    fn the_first_scan_is_saved_only_after_presentation() {
        let (mut library, jobs, _completed) = controlled(false);
        library.initial = Some(Vec::new());
        library.poll(true);
        assert!(jobs.try_recv().is_err());
        library.presented();
        assert!(matches!(jobs.try_recv(), Ok(Job::Save(0, _))));
        library.presented();
        assert!(jobs.try_recv().is_err());
    }

    #[test]
    fn completed_scans_wait_until_it_is_safe_to_replace_the_shelf() {
        let (mut library, _jobs, completed) = controlled(true);
        library.presented();
        library.poll(true);
        completed.send((0, Some(Vec::new()))).unwrap();
        assert!(library.poll(false).is_none());
        assert_eq!(library.poll(true), Some(Vec::new()));
        assert!(library.poll(true).is_none());
    }

    #[test]
    fn a_game_change_discards_an_old_scan_and_requests_a_new_one() {
        let (mut library, jobs, completed) = controlled(true);
        library.presented();
        library.poll(true);
        assert!(matches!(jobs.try_recv(), Ok(Job::Scan(0))));
        library.invalidate();
        assert!(matches!(jobs.try_recv(), Ok(Job::Remove)));
        completed.send((0, Some(Vec::new()))).unwrap();
        assert!(library.poll(true).is_none());
        assert!(matches!(jobs.try_recv(), Ok(Job::Scan(1))));
        completed.send((1, Some(Vec::new()))).unwrap();
        assert_eq!(library.poll(true), Some(Vec::new()));
    }

    #[test]
    fn a_change_also_discards_a_result_waiting_for_the_game_to_end() {
        let (mut library, _jobs, completed) = controlled(false);
        library.presented();
        completed.send((0, Some(Vec::new()))).unwrap();
        library.poll(false);
        library.invalidate();
        assert!(library.poll(true).is_none());
    }

    #[test]
    fn a_failed_scan_keeps_the_shelf_and_does_not_retry_every_frame() {
        let (mut library, jobs, completed) = controlled(true);
        library.presented();
        library.poll(true);
        assert!(matches!(jobs.try_recv(), Ok(Job::Scan(0))));
        completed.send((0, None)).unwrap();
        assert!(library.poll(true).is_none());
        assert!(jobs.try_recv().is_err());
    }

    #[test]
    fn an_obsolete_save_cannot_publish_a_cache() {
        let root = tempfile::tempdir().unwrap();
        save_current(root.path(), &AtomicU64::new(1), 0, &[]);
        assert!(!slot_store::library_cache_path(root.path()).exists());
    }
}
