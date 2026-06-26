//! QuickJS memory usage conversion.

use rquickjs::runtime::MemoryUsage;

use crate::types::VmQuickJsMemoryStats;

const BASIS_POINTS_PER_UNIT: u64 = 10_000;

pub(crate) fn quickjs_memory_stats(usage: MemoryUsage) -> VmQuickJsMemoryStats {
    let malloc_limit_bytes = non_negative_i64_to_u64(usage.malloc_limit);
    let memory_used_bytes = non_negative_i64_to_u64(usage.memory_used_size);

    VmQuickJsMemoryStats {
        malloc_size_bytes: non_negative_i64_to_u64(usage.malloc_size),
        malloc_limit_bytes,
        memory_used_bytes,
        malloc_count: non_negative_i64_to_u64(usage.malloc_count),
        atom_count: non_negative_i64_to_u64(usage.atom_count),
        string_count: non_negative_i64_to_u64(usage.str_count),
        object_count: non_negative_i64_to_u64(usage.obj_count),
        function_count: non_negative_i64_to_u64(usage.js_func_count),
        memory_pressure_bps: memory_pressure_bps(memory_used_bytes, malloc_limit_bytes),
        memory_pressure_alert: None,
    }
}

fn non_negative_i64_to_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn memory_pressure_bps(memory_used_bytes: u64, malloc_limit_bytes: u64) -> Option<u64> {
    if malloc_limit_bytes == 0 {
        return None;
    }

    Some(memory_used_bytes.saturating_mul(BASIS_POINTS_PER_UNIT) / malloc_limit_bytes)
}
