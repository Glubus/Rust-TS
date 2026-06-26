//! Process memory collection helpers.

use std::fs;

use crate::types::VmProcessMemoryStats;

const PROC_SELF_STATUS: &str = "/proc/self/status";
const VM_RSS_PREFIX: &str = "VmRSS:";
const VM_SIZE_PREFIX: &str = "VmSize:";
const KIB_BYTES: u64 = 1024;

pub(super) fn current_process_memory() -> Option<VmProcessMemoryStats> {
    let status = fs::read_to_string(PROC_SELF_STATUS).ok()?;
    parse_status_memory(&status)
}

fn parse_status_memory(status: &str) -> Option<VmProcessMemoryStats> {
    let resident_bytes = find_status_bytes(status, VM_RSS_PREFIX)?;
    let virtual_bytes = find_status_bytes(status, VM_SIZE_PREFIX)?;
    Some(VmProcessMemoryStats {
        resident_bytes,
        virtual_bytes,
    })
}

fn find_status_bytes(status: &str, prefix: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|line| parse_status_bytes_line(line, prefix))
}

fn parse_status_bytes_line(line: &str, prefix: &str) -> Option<u64> {
    let value = line.strip_prefix(prefix)?.trim();
    let kib = value.split_whitespace().next()?.parse::<u64>().ok()?;
    Some(kib * KIB_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linux_status_memory() {
        let status = "Name:\ttest\nVmSize:\t   1234 kB\nVmRSS:\t    456 kB\n";

        let memory = parse_status_memory(status).expect("memory");

        assert_eq!(memory.virtual_bytes, 1_263_616);
        assert_eq!(memory.resident_bytes, 466_944);
    }
}
