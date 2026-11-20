//! Times each part of a sample (diagnostics).
#![allow(clippy::print_stdout, clippy::unwrap_used)]
use std::time::Instant;
use sysinfo::{Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

fn main() {
    let mut sys = System::new().unwrap();
    let mut nets = Networks::new_with_refreshed_list().unwrap();
    let mut disks = Disks::new_with_refreshed_list().unwrap();
    for round in 0..4 {
        let t = Instant::now();
        sys.refresh_cpu_usage();
        let cpu = t.elapsed();
        let t = Instant::now();
        sys.refresh_memory();
        let mem = t.elapsed();
        let t = Instant::now();
        nets.refresh(true);
        let net = t.elapsed();
        let t = Instant::now();
        disks.refresh(true);
        let disk = t.elapsed();
        let t = Instant::now();
        let _ = System::uptime();
        let up = t.elapsed();
        let t = Instant::now();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_cmd(UpdateKind::OnlyIfNotSet),
        );
        let procs = t.elapsed();
        let t = Instant::now();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let procs_light = t.elapsed();
        println!(
            "round {round}: cpu {cpu:?} mem {mem:?} net {net:?} disk {disk:?} uptime {up:?} procs {procs:?} procs(cpu+mem) {procs_light:?} n={}",
            sys.processes().len()
        );
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
}
