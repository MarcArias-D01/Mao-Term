//! Sidebar panels fed by the system sampler: the agents list and the system
//! monitor. See `docs/specs/agents-panel.md` and `docs/specs/system-monitor.md`.

use mao_render::{Align, Rgb8, UiFrame};
use mao_sysmon::{HISTORY, human_bytes, human_duration};
use mao_ui::Rect;

use crate::chrome::{DrawState, Hit, KEY_AGENTS, KEY_MONITOR, mix, text};
use crate::sidebar::{AgentRow, SortBy};

/// Clickable things of the panels, recorded while drawing (hit-testing uses
/// the last frame's geometry).
#[derive(Debug, Clone, Default)]
pub struct PanelHits {
    pub sort_cpu: Rect,
    pub sort_mem: Rect,
    pub processes: Vec<(Rect, u32)>,
    /// Height of the monitor's content (for scrolling).
    pub content_h: f32,
}

impl PanelHits {
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        if self.sort_cpu.contains(x, y) {
            return Some(Hit::MonitorSort(SortBy::Cpu));
        }
        if self.sort_mem.contains(x, y) {
            return Some(Hit::MonitorSort(SortBy::Memory));
        }
        self.processes
            .iter()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, pid)| Hit::Process(*pid))
    }
}

/// Second header line of the agents panel.
pub fn agents_summary(s: &DrawState<'_>) -> String {
    let view = &s.sidebar.agents;
    if !view.scanned {
        return "Scanning processes…".into();
    }
    match view.entries.len() {
        0 => "No agents running".into(),
        1 => "1 agent running".into(),
        n => format!("{n} agents running"),
    }
}

pub fn draw_agents(ui: &mut UiFrame, s: &DrawState<'_>) {
    let (l, m, c) = (s.layout, s.metrics, s.colors);
    let view = &s.sidebar.agents;
    let area = l.sidebar_rows;
    let pad = m.px(14.0);
    if view.scanned && view.entries.is_empty() {
        let hint = "Detects Claude Code, opencode, Codex, Gemini CLI, Aider, Cursor Agent, Goose, Amp and Copilot CLI (more in the config).";
        // Wrap the hint by hand into a few short lines.
        let max_chars = ((area.w - 2.0 * pad) / (m.ui_text * 0.5)).max(10.0) as usize;
        let mut lines: Vec<String> = vec![String::new()];
        for word in hint.split(' ') {
            let last = lines.last_mut().map_or(0, |l| l.len());
            if last + word.len() + 1 > max_chars {
                lines.push(String::new());
            }
            if let Some(line) = lines.last_mut() {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
            }
        }
        for (i, line) in lines.into_iter().enumerate() {
            ui.texts.push(text(
                KEY_AGENTS | 0xFFFF00 | i as u64,
                line,
                Rect::new(
                    area.x + pad,
                    area.y + i as f32 * m.px(20.0),
                    area.w - 2.0 * pad,
                    m.px(20.0),
                ),
                m.ui_text * 0.9,
                c.dim,
                Align::Left,
            ));
        }
        return;
    }

    let rows = view.rows();
    let visible = l.visible_rows(m);
    for (slot, row) in rows.iter().enumerate().skip(view.scroll).take(visible) {
        let y = area.y + (slot - view.scroll) as f32 * m.row;
        let rr = Rect::new(area.x, y, area.w - m.gap, m.row);
        let k = KEY_AGENTS | ((slot - view.scroll) as u64) << 4;
        match *row {
            AgentRow::Group(g) => {
                let (cwd, _) = &view.groups[g];
                let (name, path) = match cwd {
                    Some(p) => (
                        p.file_name().map_or_else(
                            || p.display().to_string(),
                            |n| n.to_string_lossy().into_owned(),
                        ),
                        p.display().to_string(),
                    ),
                    None => ("Unknown folder".to_owned(), String::new()),
                };
                let name_w =
                    (name.chars().count() as f32 * m.ui_text * 0.62 + m.px(8.0)).min(rr.w * 0.6);
                let mut t = text(
                    k,
                    name,
                    Rect::new(rr.x + pad, rr.y, name_w, rr.h),
                    m.ui_text,
                    c.fg,
                    Align::Left,
                );
                t.bold = true;
                ui.texts.push(t);
                ui.texts.push(text(
                    k | 1,
                    path,
                    Rect::new(
                        rr.x + pad + name_w,
                        rr.y,
                        (rr.w - 2.0 * pad - name_w).max(0.0),
                        rr.h,
                    ),
                    m.ui_text * 0.85,
                    c.dim,
                    Align::Left,
                ));
            }
            AgentRow::Agent(i, line) => {
                let e = &view.entries[i];
                let selected = view.selected == Some(e.agent.pid);
                let hovered = matches!(s.hover, Hit::SidebarRow(h)
                    if matches!(rows.get(*h), Some(AgentRow::Agent(j, _)) if *j == i));
                if selected || hovered {
                    ui.rect(
                        rr.x,
                        rr.y,
                        rr.w,
                        rr.h,
                        if selected { c.selection } else { c.hover },
                    );
                }
                let x = rr.x + pad + m.px(10.0);
                if line == 0 {
                    // Busy agents get a bright dot.
                    let dot = if e.agent.cpu >= 0.5 {
                        c.branch
                    } else {
                        mix(c.branch, c.bg, 0.5)
                    };
                    ui.texts.push(text(
                        k,
                        "●",
                        Rect::new(x, rr.y, m.px(14.0), rr.h),
                        m.ui_text * 0.8,
                        dot,
                        Align::Left,
                    ));
                    let mut t = text(
                        k | 1,
                        e.agent.name.clone(),
                        Rect::new(x + m.px(16.0), rr.y, rr.w * 0.6, rr.h),
                        m.ui_text,
                        c.fg,
                        Align::Left,
                    );
                    t.bold = true;
                    ui.texts.push(t);
                    if let Some((_, tab)) = e.pane {
                        ui.texts.push(text(
                            k | 2,
                            format!("tab {}", tab + 1),
                            Rect::new(rr.x, rr.y, rr.w - pad, rr.h),
                            m.ui_text * 0.85,
                            c.accent,
                            Align::Right,
                        ));
                    }
                } else {
                    let a = &e.agent;
                    ui.texts.push(text(
                        k,
                        format!(
                            "pid {} · {} · {:.1}% · {}",
                            a.pid,
                            human_duration(a.run_time),
                            a.cpu,
                            human_bytes(a.memory)
                        ),
                        Rect::new(
                            x + m.px(16.0),
                            rr.y - m.px(4.0),
                            rr.w - (x - rr.x) - pad,
                            rr.h,
                        ),
                        m.ui_text * 0.85,
                        c.dim,
                        Align::Left,
                    ));
                }
            }
        }
    }
}

/// Monitor panel. Returns the clickable geometry.
pub fn draw_monitor(ui: &mut UiFrame, s: &DrawState<'_>) -> PanelHits {
    let (l, m, c) = (s.layout, s.metrics, s.colors);
    let view = &s.sidebar.monitor;
    let area = l.sidebar_rows;
    let pad = m.px(14.0);
    let mut hits = PanelHits::default();
    let Some(snap) = view.snap.as_deref() else {
        ui.texts.push(text(
            KEY_MONITOR,
            "Sampling…",
            Rect::new(area.x + pad, area.y, area.w - 2.0 * pad, m.row),
            m.ui_text,
            c.dim,
            Align::Left,
        ));
        return hits;
    };
    let inner = Rect::new(
        area.x + pad,
        area.y,
        (area.w - 2.0 * pad - m.px(4.0)).max(10.0),
        area.h,
    );
    let top = area.y - view.scroll;
    let mut y = top;
    let line = m.px(22.0);
    let graph_h = m.px(42.0);
    let gap = m.px(12.0);
    let mut key = KEY_MONITOR;
    let mut next_key = || {
        key += 1;
        key
    };
    // Only draw what is fully inside the panel.
    let shown = |y: f32, h: f32| y >= area.y - 0.5 && y + h <= area.bottom() + 0.5;

    let title = |ui: &mut UiFrame, y: f32, label: &str, value: String, color: Rgb8, key: u64| {
        if !shown(y, line) {
            return;
        }
        let mut t = text(
            key,
            label,
            Rect::new(inner.x, y, inner.w * 0.4, line),
            m.ui_text * 0.9,
            c.fg,
            Align::Left,
        );
        t.bold = true;
        ui.texts.push(t);
        ui.texts.push(text(
            key | 1 << 40,
            value,
            Rect::new(inner.x, y, inner.w, line),
            m.ui_text * 0.9,
            color,
            Align::Right,
        ));
    };

    // CPU.
    let cpu_color = level_color(c, snap.cpu, c.accent);
    let freq = if snap.frequency_mhz > 0 {
        format!(
            "{:.0}%  ·  {:.1} GHz",
            snap.cpu,
            snap.frequency_mhz as f32 / 1000.0
        )
    } else {
        format!("{:.0}%", snap.cpu)
    };
    title(ui, y, "CPU", freq, cpu_color, next_key());
    y += line;
    if shown(y, graph_h) {
        graph(
            ui,
            Rect::new(inner.x, y, inner.w, graph_h),
            &snap.cpu_history,
            100.0,
            c.accent,
            c,
        );
    }
    y += graph_h + m.px(4.0);
    // Per-core bars.
    let cores = snap.cores.len().max(1);
    let core_h = m.px(16.0);
    if shown(y, core_h) {
        let w = inner.w / cores as f32;
        for (i, u) in snap.cores.iter().enumerate() {
            let x = inner.x + i as f32 * w;
            let bw = (w - m.px(2.0)).max(1.0);
            ui.rect(x, y, bw, core_h, c.raised);
            let h = (core_h * u / 100.0).clamp(0.0, core_h);
            ui.rect(x, y + core_h - h, bw, h, level_color(c, *u, c.accent));
        }
    }
    y += core_h + gap;

    // Memory.
    let mem_pct = pct(snap.mem_used, snap.mem_total);
    title(
        ui,
        y,
        "Memory",
        format!(
            "{} / {}  ({:.0}%)",
            human_bytes(snap.mem_used),
            human_bytes(snap.mem_total),
            mem_pct
        ),
        level_color(c, mem_pct, c.option),
        next_key(),
    );
    y += line;
    if shown(y, graph_h) {
        graph(
            ui,
            Rect::new(inner.x, y, inner.w, graph_h),
            &snap.mem_history,
            100.0,
            c.option,
            c,
        );
    }
    y += graph_h + m.px(6.0);
    if snap.swap_total > 0 {
        let swap_pct = pct(snap.swap_used, snap.swap_total);
        let label = if cfg!(windows) { "Page file" } else { "Swap" };
        title(
            ui,
            y,
            label,
            format!(
                "{} / {}",
                human_bytes(snap.swap_used),
                human_bytes(snap.swap_total)
            ),
            c.dim,
            next_key(),
        );
        y += line;
        if shown(y, m.px(6.0)) {
            bar(
                ui,
                Rect::new(inner.x, y, inner.w, m.px(6.0)),
                swap_pct,
                c.option,
                c,
            );
        }
        y += m.px(6.0);
    }
    y += gap;

    // Network.
    title(
        ui,
        y,
        "Network",
        format!(
            "↓ {}/s   ↑ {}/s",
            human_bytes(snap.net_rx),
            human_bytes(snap.net_tx)
        ),
        c.dim,
        next_key(),
    );
    y += line;
    if shown(y, graph_h) {
        let max = snap
            .net_rx_history
            .iter()
            .chain(&snap.net_tx_history)
            .copied()
            .max()
            .unwrap_or(0)
            .max(64 * 1024) as f32;
        let r = Rect::new(inner.x, y, inner.w, graph_h);
        let rx: Vec<f32> = snap.net_rx_history.iter().map(|v| *v as f32).collect();
        let tx: Vec<f32> = snap.net_tx_history.iter().map(|v| *v as f32).collect();
        graph(ui, r, &rx, max, c.branch, c);
        lines(ui, r, &tx, max, c.warn);
    }
    y += graph_h + gap;

    // Disks.
    for d in &snap.disks {
        if d.total == 0 {
            continue;
        }
        let used = d.total - d.available.min(d.total);
        let p = pct(used, d.total);
        title(
            ui,
            y,
            &d.mount,
            format!("{} / {}", human_bytes(used), human_bytes(d.total)),
            level_color(c, p, c.dim),
            next_key(),
        );
        y += line;
        if shown(y, m.px(6.0)) {
            bar(
                ui,
                Rect::new(inner.x, y, inner.w, m.px(6.0)),
                p,
                c.accent,
                c,
            );
        }
        y += m.px(6.0) + m.px(8.0);
    }
    y += gap - m.px(8.0);

    // System.
    title(
        ui,
        y,
        "Uptime",
        human_duration(snap.uptime),
        c.dim,
        next_key(),
    );
    y += line;
    if shown(y, line) {
        ui.texts.push(text(
            next_key(),
            snap.os.clone(),
            Rect::new(inner.x, y, inner.w, line),
            m.ui_text * 0.85,
            c.dim,
            Align::Left,
        ));
    }
    y += line + gap;

    // Top processes.
    let col_mem = m.px(70.0);
    let col_cpu = m.px(52.0);
    let header = Rect::new(inner.x, y, inner.w, line);
    if shown(y, line) {
        let mut t = text(
            next_key(),
            "Processes",
            header,
            m.ui_text * 0.9,
            c.fg,
            Align::Left,
        );
        t.bold = true;
        ui.texts.push(t);
        let mem_r = Rect::new(header.right() - col_mem, y, col_mem, line);
        let cpu_r = Rect::new(mem_r.x - col_cpu, y, col_cpu, line);
        for (r, label, key, sort) in [
            (cpu_r, "CPU", next_key(), SortBy::Cpu),
            (mem_r, "Memory", next_key(), SortBy::Memory),
        ] {
            let active = view.sort == sort;
            if *s.hover == Hit::MonitorSort(sort) {
                ui.rect(r.x, r.y, r.w, r.h, c.hover);
            }
            let label = if active {
                format!("{label} ▾")
            } else {
                label.to_owned()
            };
            ui.texts.push(text(
                key,
                label,
                r,
                m.ui_text * 0.85,
                if active { c.accent } else { c.dim },
                Align::Right,
            ));
        }
        hits.sort_cpu = cpu_r;
        hits.sort_mem = mem_r;
    }
    y += line;
    for (i, p) in view.top().into_iter().enumerate() {
        let r = Rect::new(area.x, y, area.w - m.gap, line);
        if shown(y, line) {
            let selected = view.selected == Some(p.pid);
            if selected {
                ui.rect(r.x, r.y, r.w, r.h, c.selection);
            } else if *s.hover == Hit::Process(p.pid) {
                ui.rect(r.x, r.y, r.w, r.h, c.hover);
            }
            let k = KEY_MONITOR | 0x1000 | (i as u64) << 4;
            let name_w = (inner.w - col_mem - col_cpu - m.px(4.0)).max(0.0);
            ui.texts.push(text(
                k,
                p.name.clone(),
                Rect::new(inner.x, y, name_w, line),
                m.ui_text * 0.88,
                c.fg,
                Align::Left,
            ));
            ui.texts.push(text(
                k | 1,
                format!("{:.1}%", p.cpu),
                Rect::new(inner.right() - col_mem - col_cpu, y, col_cpu, line),
                m.ui_text * 0.85,
                level_color(c, p.cpu * 4.0, c.dim),
                Align::Right,
            ));
            ui.texts.push(text(
                k | 2,
                human_bytes(p.memory),
                Rect::new(inner.right() - col_mem, y, col_mem, line),
                m.ui_text * 0.85,
                c.dim,
                Align::Right,
            ));
            hits.processes.push((r, p.pid));
        }
        y += line;
    }
    if s.layout.sidebar.is_some() {
        let footer = format!("sample {:.1} ms", snap.cost.as_secs_f64() * 1000.0);
        y += m.px(4.0);
        if shown(y, line) {
            ui.texts.push(text(
                next_key(),
                footer,
                Rect::new(inner.x, y, inner.w, line),
                m.ui_text * 0.8,
                mix(c.dim, c.bg, 0.3),
                Align::Left,
            ));
        }
        y += line;
    }
    hits.content_h = y - top + m.px(8.0);
    hits
}

fn pct(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        used as f32 * 100.0 / total as f32
    }
}

/// Normal color, or warning/error colors at high usage.
fn level_color(c: &crate::chrome::Colors, pct: f32, normal: Rgb8) -> Rgb8 {
    if pct >= 90.0 {
        c.error
    } else if pct >= 75.0 {
        c.warn
    } else {
        normal
    }
}

/// History as vertical bars, newest on the right.
fn graph(
    ui: &mut UiFrame,
    r: Rect,
    values: &[f32],
    max: f32,
    color: Rgb8,
    c: &crate::chrome::Colors,
) {
    ui.rect(r.x, r.y, r.w, r.h, c.raised);
    let w = r.w / HISTORY as f32;
    let fill = mix(color, c.raised, 0.45);
    let n = values.len();
    for (i, v) in values.iter().enumerate() {
        let h = (r.h * v / max.max(f32::EPSILON)).clamp(0.0, r.h);
        if h < 0.5 {
            continue;
        }
        let x = r.right() - (n - i) as f32 * w;
        ui.rect(x, r.bottom() - h, (w - 0.5).max(1.0), h, fill);
        // A brighter cap marks the value.
        ui.rect(x, r.bottom() - h, (w - 0.5).max(1.0), 1.5_f32.min(h), color);
    }
}

/// History as markers only (overlaid on a graph).
fn lines(ui: &mut UiFrame, r: Rect, values: &[f32], max: f32, color: Rgb8) {
    let w = r.w / HISTORY as f32;
    let n = values.len();
    for (i, v) in values.iter().enumerate() {
        let h = (r.h * v / max.max(f32::EPSILON)).clamp(0.0, r.h);
        let x = r.right() - (n - i) as f32 * w;
        ui.rect(x, r.bottom() - h - 1.0, (w - 0.5).max(1.0), 2.0, color);
    }
}

fn bar(ui: &mut UiFrame, r: Rect, pct: f32, color: Rgb8, c: &crate::chrome::Colors) {
    ui.rect(r.x, r.y, r.w, r.h, c.raised);
    let w = (r.w * pct / 100.0).clamp(0.0, r.w);
    ui.rect(r.x, r.y, w, r.h, level_color(c, pct, color));
}
