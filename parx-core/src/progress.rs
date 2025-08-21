use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Progress {
    enabled: bool,
    pub stage: Arc<Mutex<String>>,
    pub blocks_done: Arc<AtomicUsize>,
    pub blocks_total: Arc<AtomicUsize>,
    pub bytes_done: Arc<AtomicUsize>,
    pub bytes_total: Arc<AtomicUsize>,
    running: Arc<AtomicBool>,
}

impl Progress {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            stage: Arc::new(Mutex::new(String::new())),
            blocks_done: Arc::new(AtomicUsize::new(0)),
            blocks_total: Arc::new(AtomicUsize::new(0)),
            bytes_done: Arc::new(AtomicUsize::new(0)),
            bytes_total: Arc::new(AtomicUsize::new(0)),
            running: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn set_stage(&self, s: &str) {
        if self.enabled {
            *self.stage.lock().unwrap() = s.to_string();
        }
    }
    pub fn set_blocks_total(&self, n: usize) {
        self.blocks_total.store(n, Ordering::Relaxed);
    }
    pub fn inc_block(&self) {
        self.blocks_done.fetch_add(1, Ordering::Relaxed);
    }
    pub fn reset_bytes(&self, total: usize) {
        self.bytes_total.store(total, Ordering::Relaxed);
        self.bytes_done.store(0, Ordering::Relaxed);
    }
    pub fn add_bytes(&self, n: usize) {
        self.bytes_done.fetch_add(n, Ordering::Relaxed);
    }

    pub fn start(&self) {
        if !self.enabled {
            return;
        }
        self.running.store(true, Ordering::Relaxed);
        let stage = self.stage.clone();
        let blocks_done = self.blocks_done.clone();
        let blocks_total = self.blocks_total.clone();
        let bytes_done = self.bytes_done.clone();
        let bytes_total = self.bytes_total.clone();
        let running = self.running.clone();
        thread::spawn(move || {
            let t0 = Instant::now();
            while running.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_secs(5));
                if !running.load(Ordering::Relaxed) {
                    break;
                }
                let s = stage.lock().unwrap().clone();
                let bd = blocks_done.load(Ordering::Relaxed);
                let bt = blocks_total.load(Ordering::Relaxed);
                let bpd = bytes_done.load(Ordering::Relaxed);
                let bpt = bytes_total.load(Ordering::Relaxed);
                let bpct = if bpt > 0 { (bpd as f64 / bpt as f64) * 100.0 } else { 0.0 };
                eprintln!(
                    "[{:>4}s] {} | stripes {}/{} | bytes {}%",
                    t0.elapsed().as_secs(),
                    s,
                    bd,
                    bt,
                    bpct as i32
                );
            }
        });
    }
    pub fn stop(&self) {
        if self.enabled {
            self.running.store(false, Ordering::Relaxed);
        }
    }
}
