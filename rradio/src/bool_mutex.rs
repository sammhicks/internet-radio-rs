use std::sync::atomic::{AtomicBool, Ordering};

pub struct BoolMutex(AtomicBool);

impl BoolMutex {
    pub const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    pub fn lock(&self) -> Lock<'_> {
        while self.0.swap(true, Ordering::SeqCst) {
            std::hint::spin_loop();
        }

        Lock(self)
    }
}

pub struct Lock<'a>(&'a BoolMutex);

impl<'a> std::ops::Drop for Lock<'a> {
    fn drop(&mut self) {
        self.0 .0.store(false, Ordering::SeqCst);
    }
}
