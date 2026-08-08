//! Process memory reporting.
//!
//! Wreath keeps a replay resident, so its footprint is a feature people judge
//! it on. Reporting it is the only way to tell the part that is inherent - the
//! encoded buffer itself - from the part that is waste.

/// Private bytes this process has committed, which is the number Task Manager
/// shows under Memory.
#[cfg(target_os = "windows")]
pub fn private_bytes() -> Option<u64> {
    use windows::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS_EX2::default();
    let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32;
    unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX2).cast::<PROCESS_MEMORY_COUNTERS>(),
            size,
        )
    }
    .ok()?;
    Some(counters.PrivateUsage as u64)
}

#[cfg(not(target_os = "windows"))]
pub fn private_bytes() -> Option<u64> {
    None
}

/// Logs a labelled footprint line, rounded to whole megabytes.
pub fn report(component: &str, detail: &str) {
    let Some(bytes) = private_bytes() else {
        return;
    };
    wreath_core::diagnostic!(
        "Wreath {component} memory: {} MB private{}{detail}",
        bytes / 1_048_576,
        if detail.is_empty() { "" } else { ", " }
    );
}
