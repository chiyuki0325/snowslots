use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use slot_store::{Cart, Platform};
use slot_ui::{cart_face, CartFace};

pub struct BuiltCartFace {
    pub platform: Platform,
    pub stem: String,
    pub face: CartFace,
}

pub struct LibraryFaceBuilder {
    requests: Sender<Vec<Cart>>,
    built: Receiver<BuiltCartFace>,
}

impl LibraryFaceBuilder {
    pub fn spawn() -> Self {
        let (requests, inbox) = mpsc::channel::<Vec<Cart>>();
        let (outbox, built) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("slot-library-faces".into())
            .spawn(move || {
                while let Ok(mut carts) = inbox.recv() {
                    while let Ok(newer) = inbox.try_recv() {
                        carts = newer;
                    }
                    for cart in carts {
                        let face = BuiltCartFace {
                            platform: cart.platform,
                            stem: cart.stem.clone(),
                            face: cart_face(&cart),
                        };
                        if outbox.send(face).is_err() {
                            return;
                        }
                    }
                }
            });
        if let Err(e) = spawned {
            eprintln!("slot: library faces: worker thread failed to start: {e}");
        }
        LibraryFaceBuilder { requests, built }
    }

    pub fn request(&self, carts: Vec<Cart>) {
        let _ = self.requests.send(carts);
    }

    pub fn take(&self) -> Option<BuiltCartFace> {
        self.built.try_recv().ok()
    }
}
