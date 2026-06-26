use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ts_embed_vm::VmOptions;

static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct TestCacheDir {
    path: PathBuf,
}

impl TestCacheDir {
    pub(crate) fn new(prefix: &str) -> Self {
        let path = unique_path(prefix);
        fs::create_dir_all(&path).expect("create test cache dir");
        Self { path }
    }

    pub(crate) fn vm_options(&self) -> VmOptions {
        VmOptions {
            worker_threads: 1,
            cache_dir: self.path.clone(),
            max_scripts_per_worker: 8,
            queue_capacity: 32,
            idle_sleep: std::time::Duration::from_millis(5),
            memory_limit_bytes: 16 * 1024 * 1024,
            max_stack_size_bytes: 512 * 1024,
            memory_pressure_thresholds: None,
            latency_histograms: false,
            contract_validation: Default::default(),
            unknown_field_validation: Default::default(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TestCacheDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn unique_path(prefix: &str) -> PathBuf {
    let sequence = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ts-embed-vm-{prefix}-{}-{timestamp}-{sequence}",
        std::process::id()
    ))
}
