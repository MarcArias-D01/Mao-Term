//! System metrics sampling. See docs/specs/system-monitor.md.
//!
//! A [`Sampler`] owns a worker thread that samples with `sysinfo` only while
//! somebody is interested (a panel is open); otherwise it blocks on its channel
//! and costs nothing.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use sysinfo::{Disks, Networks, Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Samples kept for the graphs (one per second).
pub const HISTORY: usize = 60;

#[derive(Debug, Clone, PartialEq)]
pub struct ProcInfo {
    pub pid: u32,
    pub parent: Option<u32>,
    pub name: String,
    pub exe: Option<PathBuf>,
    pub cmd: Vec<String>,
    /// Only filled for processes accepted by the detail filter.
    pub cwd: Option<PathBuf>,
    /// Percentage of the whole machine (0–100), like Task Manager.
    pub cpu: f32,
    /// Resident memory in bytes.
    pub memory: u64,
    /// Seconds since the process started.
    pub run_time: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiskInfo {
    pub name: String,
    pub mount: String,
    pub total: u64,
    pub available: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    /// Total CPU usage, 0–100.
    pub cpu: f32,
    pub cores: Vec<f32>,
    pub frequency_mhz: u64,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub disks: Vec<DiskInfo>,
    /// Network bytes per second, all interfaces.
    pub net_rx: u64,
    pub net_tx: u64,
    /// Oldest first, at most [`HISTORY`] values each.
    pub cpu_history: Vec<f32>,
    pub mem_history: Vec<f32>,
    pub net_rx_history: Vec<u64>,
    pub net_tx_history: Vec<u64>,
    pub uptime: u64,
    pub os: String,
    pub host: String,
    /// Present when processes were requested.
    pub processes: Option<Arc<Vec<ProcInfo>>>,
    /// How long this sample took to collect.
    pub cost: Duration,
}

/// What the UI currently needs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Interest {
    /// CPU, memory, disks, network (the monitor panel).
    pub metrics: bool,
    /// The process list (monitor's top processes, agents).
    pub processes: bool,
}

impl Interest {
    pub fn any(self) -> bool {
        self.metrics || self.processes
    }
}

/// Decides which processes get their working directory read (it is slow to
/// read for every process on Windows).
pub type DetailFilter =
    Arc<dyn Fn(&str, Option<&std::path::Path>, &[String]) -> bool + Send + Sync>;

enum Msg {
    Interest(Interest),
    Filter(Option<DetailFilter>),
    Kill(u32),
    Refresh,
}

pub struct Sampler {
    tx: Sender<Msg>,
    interest: Interest,
}

impl Sampler {
    /// Starts the worker (idle until [`Self::set_interest`]).
    pub fn new(notify: impl Fn(Arc<Snapshot>) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel::<Msg>();
        let _ = std::thread::Builder::new()
            .name("sysmon".into())
            .spawn(move || Worker::new().run(&rx, &notify));
        Self {
            tx,
            interest: Interest::default(),
        }
    }

    pub fn set_interest(&mut self, interest: Interest) {
        if interest != self.interest {
            self.interest = interest;
            let _ = self.tx.send(Msg::Interest(interest));
        }
    }

    pub fn interest(&self) -> Interest {
        self.interest
    }

    pub fn set_detail_filter(&self, filter: Option<DetailFilter>) {
        let _ = self.tx.send(Msg::Filter(filter));
    }

    /// Kills a process, then samples again.
    pub fn kill(&self, pid: u32) {
        let _ = self.tx.send(Msg::Kill(pid));
    }

    /// Samples now (e.g. after a configuration change).
    pub fn refresh(&self) {
        let _ = self.tx.send(Msg::Refresh);
    }
}

struct Worker {
    sys: Option<System>,
    disks: Option<Disks>,
    networks: Option<Networks>,
    interest: Interest,
    filter: Option<DetailFilter>,
    cpu_history: VecDeque<f32>,
    mem_history: VecDeque<f32>,
    rx_history: VecDeque<u64>,
    tx_history: VecDeque<u64>,
    last_net: Option<Instant>,
    tick: u64,
    procs: Option<Arc<Vec<ProcInfo>>>,
    /// OS and host names (read once: they need registry/system calls).
    names: Option<(String, String)>,
    /// Last measured network rates (rx, tx) in bytes per second.
    net_rate: (u64, u64),
}

impl Worker {
    fn new() -> Self {
        Self {
            sys: None,
            disks: None,
            networks: None,
            interest: Interest::default(),
            filter: None,
            cpu_history: VecDeque::new(),
            mem_history: VecDeque::new(),
            rx_history: VecDeque::new(),
            tx_history: VecDeque::new(),
            last_net: None,
            tick: 0,
            procs: None,
            names: None,
            net_rate: (0, 0),
        }
    }

    fn run(mut self, rx: &mpsc::Receiver<Msg>, notify: &dyn Fn(Arc<Snapshot>)) {
        loop {
            // Idle: block until somebody is interested.
            if !self.interest.any() {
                match rx.recv() {
                    Ok(msg) => self.handle(msg),
                    Err(_) => return,
                }
                if !self.interest.any() {
                    // Free what sysinfo holds while nobody looks.
                    self.sys = None;
                    self.disks = None;
                    self.networks = None;
                    self.procs = None;
                    self.last_net = None;
                    continue;
                }
            }
            notify(Arc::new(self.sample()));
            self.tick += 1;
            // Metrics refresh every second; the process list alone every 3 s.
            let period = if self.interest.metrics {
                Duration::from_secs(1)
            } else {
                Duration::from_secs(3)
            };
            let deadline = Instant::now() + period;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(left) {
                    Ok(Msg::Refresh) => break,
                    Ok(msg) => {
                        let kill = matches!(msg, Msg::Kill(_));
                        self.handle(msg);
                        if kill || !self.interest.any() {
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Interest(i) => self.interest = i,
            Msg::Filter(f) => self.filter = f,
            Msg::Kill(pid) => {
                if let Some(sys) = self.sys.as_ref()
                    && let Some(p) = sys.process(Pid::from_u32(pid))
                {
                    let ok = p.kill();
                    tracing::info!(pid, ok, "kill process");
                }
            }
            Msg::Refresh => {}
        }
    }

    fn sample(&mut self) -> Snapshot {
        let started = Instant::now();
        if self.sys.is_none() {
            self.sys = System::new()
                .map_err(|e| tracing::warn!("system information unavailable: {e}"))
                .ok();
        }
        let mut snap = Snapshot::default();
        let Some(sys) = self.sys.as_mut() else {
            return snap;
        };

        // CPU usage needs two refreshes; also needed for per-process CPU.
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        snap.cpu = sys.global_cpu_usage();
        snap.cores = sys.cpus().iter().map(sysinfo::Cpu::usage).collect();
        snap.frequency_mhz = sys.cpus().first().map_or(0, sysinfo::Cpu::frequency);
        snap.mem_used = sys.used_memory();
        snap.mem_total = sys.total_memory();
        snap.swap_used = sys.used_swap();
        snap.swap_total = sys.total_swap();
        snap.uptime = System::uptime().unwrap_or(0);
        let (os, host) = self.names.get_or_insert_with(|| {
            (
                System::long_os_version()
                    .or_else(|_| System::name())
                    .unwrap_or_default(),
                System::host_name().unwrap_or_default(),
            )
        });
        snap.os.clone_from(os);
        snap.host.clone_from(host);

        if self.interest.metrics {
            if self.disks.is_none() {
                self.disks = Disks::new_with_refreshed_list().ok();
            } else if self.tick.is_multiple_of(10)
                && let Some(d) = self.disks.as_mut()
            {
                d.refresh(true);
            }
            snap.disks = self
                .disks
                .iter()
                .flat_map(|d| d.iter())
                .map(|d| DiskInfo {
                    name: d.name().to_string_lossy().into_owned(),
                    mount: d.mount_point().display().to_string(),
                    total: d.total_space(),
                    available: d.available_space(),
                })
                .collect();
            // Reading the interface table costs ~8 ms on Windows: every other second.
            if self.networks.is_none() {
                self.networks = Networks::new_with_refreshed_list().ok();
                self.last_net = Some(Instant::now());
            } else if self.tick.is_multiple_of(2)
                && let Some(n) = self.networks.as_mut()
            {
                n.refresh(true);
                let elapsed = self
                    .last_net
                    .map_or(1.0, |t| t.elapsed().as_secs_f64().max(0.001));
                let (rx, tx) = n.values().fold((0u64, 0u64), |(r, t), n| {
                    (r + n.received(), t + n.transmitted())
                });
                self.net_rate = ((rx as f64 / elapsed) as u64, (tx as f64 / elapsed) as u64);
                self.last_net = Some(Instant::now());
            }
            (snap.net_rx, snap.net_tx) = self.net_rate;
        }

        push(&mut self.cpu_history, snap.cpu);
        let mem_pct = if snap.mem_total > 0 {
            snap.mem_used as f32 * 100.0 / snap.mem_total as f32
        } else {
            0.0
        };
        push(&mut self.mem_history, mem_pct);
        push(&mut self.rx_history, snap.net_rx);
        push(&mut self.tx_history, snap.net_tx);
        snap.cpu_history = self.cpu_history.iter().copied().collect();
        snap.mem_history = self.mem_history.iter().copied().collect();
        snap.net_rx_history = self.rx_history.iter().copied().collect();
        snap.net_tx_history = self.tx_history.iter().copied().collect();

        // The process list is by far the most expensive part of a sample: with the
        // monitor open it refreshes every third second.
        let refresh_procs = self.interest.processes
            && (!self.interest.metrics || self.tick.is_multiple_of(3) || self.procs.is_none());
        if refresh_procs {
            self.procs = Some(Arc::new(processes(sys, self.filter.as_ref())));
        }
        if self.interest.processes {
            snap.processes.clone_from(&self.procs);
        }
        snap.cost = started.elapsed();
        tracing::trace!(
            ms = snap.cost.as_secs_f64() * 1000.0,
            refresh_procs,
            "sysmon sample"
        );
        snap
    }
}

fn processes(sys: &mut System, filter: Option<&DetailFilter>) -> Vec<ProcInfo> {
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_cmd(UpdateKind::OnlyIfNotSet),
    );
    // Working directories only for the interesting few (agents).
    if let Some(filter) = filter {
        let wanted: Vec<Pid> = sys
            .processes()
            .values()
            .filter(|p| {
                let cmd: Vec<String> = p
                    .cmd()
                    .iter()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect();
                filter(&p.name().to_string_lossy(), p.exe(), &cmd)
            })
            .map(sysinfo::Process::pid)
            .collect();
        if !wanted.is_empty() {
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&wanted),
                false,
                ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always),
            );
        }
    }
    let cores = sys.cpus().len().max(1) as f32;
    sys.processes()
        .values()
        .map(|p| ProcInfo {
            pid: p.pid().as_u32(),
            parent: p.parent().map(Pid::as_u32),
            name: p.name().to_string_lossy().into_owned(),
            exe: p.exe().map(std::path::Path::to_path_buf),
            cmd: p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
            cwd: p.cwd().map(std::path::Path::to_path_buf),
            cpu: p.cpu_usage() / cores,
            memory: p.memory(),
            run_time: p.run_time(),
        })
        .collect()
}

fn push<T>(ring: &mut VecDeque<T>, value: T) {
    if ring.len() == HISTORY {
        ring.pop_front();
    }
    ring.push_back(value);
}

/// `1.5 GB`-style size.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[unit])
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

/// `3 h 12 min`-style duration.
pub fn human_duration(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60);
    if d > 0 {
        format!("{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else if m > 0 {
        format!("{m} min")
    } else {
        format!("{secs} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KB");
        assert_eq!(human_bytes(300 * 1024 * 1024), "300 MB");
        assert_eq!(human_duration(42), "42 s");
        assert_eq!(human_duration(3 * 3600 + 12 * 60), "3 h 12 min");
        assert_eq!(human_duration(2 * 86_400 + 3600), "2 d 1 h");
    }

    #[test]
    fn history_is_bounded() {
        let mut ring = VecDeque::new();
        for i in 0..100 {
            push(&mut ring, i);
        }
        assert_eq!(ring.len(), HISTORY);
        assert_eq!(ring.front(), Some(&40));
    }

    #[test]
    fn samples_only_when_interested() {
        let (tx, rx) = mpsc::channel();
        let mut s = Sampler::new(move |snap| {
            let _ = tx.send(snap);
        });
        assert!(
            rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "sampled while idle"
        );
        s.set_interest(Interest {
            metrics: true,
            processes: true,
        });
        let snap = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(snap.mem_total > 0);
        let me = std::process::id();
        let procs = snap.processes.as_ref().unwrap();
        assert!(procs.iter().any(|p| p.pid == me), "own process missing");
        s.set_interest(Interest::default());
        // Drain what was in flight, then nothing more arrives.
        while rx.recv_timeout(Duration::from_millis(1500)).is_ok() {}
        assert!(rx.recv_timeout(Duration::from_millis(1500)).is_err());
    }
}
