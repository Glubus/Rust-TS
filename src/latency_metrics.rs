//! Low-overhead latency counters for VM operations.

use std::array;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::types::{VmLatencyHistogramBucket, VmLatencyStats};

const LATENCY_BUCKET_UPPER_BOUNDS_NS: &[u64] = &[
    10_000,
    50_000,
    100_000,
    500_000,
    1_000_000,
    5_000_000,
    10_000_000,
    50_000_000,
    100_000_000,
];
const LATENCY_BUCKET_COUNT: usize = LATENCY_BUCKET_UPPER_BOUNDS_NS.len() + 1;

/// Atomic operation latency counters.
#[derive(Debug)]
pub(crate) struct LatencyMetrics {
    load: OperationLatencyMetrics,
    call: OperationLatencyMetrics,
    emit: OperationLatencyMetrics,
}

impl LatencyMetrics {
    pub(crate) fn new(histograms_enabled: bool) -> Self {
        Self {
            load: OperationLatencyMetrics::new(histograms_enabled),
            call: OperationLatencyMetrics::new(histograms_enabled),
            emit: OperationLatencyMetrics::new(histograms_enabled),
        }
    }

    pub(crate) fn observe_load(&self, elapsed: Duration) {
        self.load.observe(elapsed);
    }

    pub(crate) fn observe_call(&self, elapsed: Duration) {
        self.call.observe(elapsed);
    }

    pub(crate) fn observe_emit(&self, elapsed: Duration) {
        self.emit.observe(elapsed);
    }

    pub(crate) fn snapshot(&self) -> VmLatencyStats {
        let load = self.load.snapshot();
        let call = self.call.snapshot();
        let emit = self.emit.snapshot();

        VmLatencyStats {
            load_operations: load.operations,
            load_total_ns: load.total_ns,
            load_average_ns: load.average_ns,
            load_max_ns: load.max_ns,
            load_histogram: load.histogram,
            call_operations: call.operations,
            call_total_ns: call.total_ns,
            call_average_ns: call.average_ns,
            call_max_ns: call.max_ns,
            call_histogram: call.histogram,
            emit_operations: emit.operations,
            emit_total_ns: emit.total_ns,
            emit_average_ns: emit.average_ns,
            emit_max_ns: emit.max_ns,
            emit_histogram: emit.histogram,
        }
    }
}

impl Default for LatencyMetrics {
    fn default() -> Self {
        Self::new(false)
    }
}

#[derive(Debug)]
struct OperationLatencyMetrics {
    histograms_enabled: bool,
    operations: AtomicU64,
    total_ns: AtomicU64,
    max_ns: AtomicU64,
    histogram: [AtomicU64; LATENCY_BUCKET_COUNT],
}

impl OperationLatencyMetrics {
    fn new(histograms_enabled: bool) -> Self {
        Self {
            histograms_enabled,
            operations: AtomicU64::new(0),
            total_ns: AtomicU64::new(0),
            max_ns: AtomicU64::new(0),
            histogram: array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    fn observe(&self, elapsed: Duration) {
        let elapsed_ns = duration_ns(elapsed);
        self.operations.fetch_add(1, Ordering::Relaxed);
        self.total_ns.fetch_add(elapsed_ns, Ordering::Relaxed);
        update_max(&self.max_ns, elapsed_ns);

        if self.histograms_enabled {
            self.histogram[bucket_index(elapsed_ns)].fetch_add(1, Ordering::Relaxed);
        }
    }

    fn snapshot(&self) -> OperationLatencySnapshot {
        let operations = self.operations.load(Ordering::Relaxed);
        let total_ns = self.total_ns.load(Ordering::Relaxed);

        OperationLatencySnapshot {
            operations,
            total_ns,
            average_ns: average_ns(total_ns, operations),
            max_ns: self.max_ns.load(Ordering::Relaxed),
            histogram: self.histograms_enabled.then(|| self.histogram_snapshot()),
        }
    }

    fn histogram_snapshot(&self) -> Vec<VmLatencyHistogramBucket> {
        let mut buckets = Vec::with_capacity(LATENCY_BUCKET_COUNT);
        for (index, counter) in self.histogram.iter().enumerate() {
            buckets.push(VmLatencyHistogramBucket {
                upper_bound_ns: LATENCY_BUCKET_UPPER_BOUNDS_NS.get(index).copied(),
                count: counter.load(Ordering::Relaxed),
            });
        }
        buckets
    }
}

struct OperationLatencySnapshot {
    operations: u64,
    total_ns: u64,
    average_ns: u64,
    max_ns: u64,
    histogram: Option<Vec<VmLatencyHistogramBucket>>,
}

fn duration_ns(elapsed: Duration) -> u64 {
    elapsed.as_nanos().try_into().unwrap_or(u64::MAX)
}

fn average_ns(total_ns: u64, operations: u64) -> u64 {
    if operations == 0 {
        return 0;
    }
    total_ns / operations
}

fn bucket_index(elapsed_ns: u64) -> usize {
    LATENCY_BUCKET_UPPER_BOUNDS_NS
        .iter()
        .position(|upper_bound| elapsed_ns <= *upper_bound)
        .unwrap_or(LATENCY_BUCKET_COUNT - 1)
}

fn update_max(max_ns: &AtomicU64, observed_ns: u64) {
    let mut current = max_ns.load(Ordering::Relaxed);
    while observed_ns > current {
        match max_ns.compare_exchange_weak(
            current,
            observed_ns,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(next) => current = next,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::LatencyMetrics;

    #[test]
    fn latency_histograms_are_disabled_by_default() {
        let metrics = LatencyMetrics::default();

        metrics.observe_call(Duration::from_micros(42));
        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.call_operations, 1);
        assert!(snapshot.call_histogram.is_none());
    }

    #[test]
    fn latency_histograms_count_observations_when_enabled() {
        let metrics = LatencyMetrics::new(true);

        metrics.observe_call(Duration::from_nanos(5_000));
        metrics.observe_call(Duration::from_millis(200));
        let snapshot = metrics.snapshot();
        let histogram = snapshot.call_histogram.expect("call histogram");

        assert_eq!(snapshot.call_operations, 2);
        assert_eq!(histogram.iter().map(|bucket| bucket.count).sum::<u64>(), 2);
        assert_eq!(histogram[0].count, 1);
        assert_eq!(histogram.last().expect("overflow bucket").count, 1);
    }
}
