//! sysinfo — CPU, memory, kernel heap, disk I/O, uptime, and process table.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn main() {
    if let Some(p) = rt::perf() {
        let total_ticks = p.busy_ticks + p.idle_ticks;
        let busy_pct = if total_ticks > 0 { p.busy_ticks * 100 / total_ticks } else { 0 };
        let used_mib = p.phys_total_frames.saturating_sub(p.phys_free_frames) * 4 / 1024;
        let total_mib = p.phys_total_frames * 4 / 1024;
        let slab_total = p.slab_hits + p.slab_misses;
        let slab_pct = if slab_total > 0 { p.slab_hits * 100 / slab_total } else { 100 };
        rt::print_args(format_args!(
            "cpu: {}% busy ({} MHz, {} syscalls, {} ctx_switches)\n",
            busy_pct, p.tsc_khz / 1000, p.syscalls, p.context_switches
        ));
        rt::print_args(format_args!(
            "ram: {} / {} MiB used ({} free frames of {})\n",
            used_mib, total_mib, p.phys_free_frames, p.phys_total_frames
        ));
        rt::print_args(format_args!(
            "heap: {} / {} KiB ({}% slab hit)\n",
            p.heap_used_bytes / 1024, p.heap_capacity_bytes / 1024, slab_pct
        ));
        rt::print_args(format_args!(
            "disk: read {} KiB ({} ops), write {} KiB ({} ops), {} flushes\n",
            p.disk_read_bytes / 1024, p.disk_reads,
            p.disk_write_bytes / 1024, p.disk_writes, p.disk_flushes
        ));
        rt::print_args(format_args!(
            "uptime: {} s ({} ticks at 100 Hz)\n",
            p.ticks / 100, p.ticks
        ));
    } else {
        rt::print_args(format_args!("free frames: {}\n", rt::call(SYS_FREE_FRAMES, 0, 0)));
        rt::print_args(format_args!("tasks: {}\n", rt::call(SYS_TASK_COUNT, 0, 0)));
        let ticks = rt::call(SYS_TICKS, 0, 0);
        rt::print_args(format_args!("ticks: {ticks}\n"));
        rt::print_args(format_args!("uptime: {} seconds (at 100 ticks/s)\n", ticks / 100));
    }
    if let Ok(mut procs) = rt::processes() {
        procs.sort_by_key(|t| t.pid);
        rt::print("PID STATE    CPU_T  RAM_KIB SYSCALLS NAME\n");
        for t in procs {
            rt::print_args(format_args!(
                "{:<3} {:<8} {:<6} {:<7} {:<8} {}\n",
                t.pid, t.state_name(), t.cpu_ticks, t.mem_frames * 4, t.syscalls, t.name()
            ));
        }
    }
    rt::exit(0);
}
