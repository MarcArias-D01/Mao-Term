//! Detection of AI agent sessions running on the machine. See docs/specs/agents-panel.md.
//!
//! Works on the process list sampled by `mao-sysmon`: a process is an agent if
//! its executable name or its command line matches a [`Detector`]. Child
//! processes of an agent (MCP servers, workers…) are folded into it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mao_sysmon::{DetailFilter, ProcInfo};

/// How to recognize one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detector {
    pub name: String,
    /// Executable names (without `.exe`), compared case-insensitively.
    pub exe: Vec<String>,
    /// Substrings of any command-line argument (for agents running under node/python).
    pub cmdline: Vec<String>,
    /// Substrings of the executable path or arguments that rule a process out.
    pub exclude: Vec<String>,
    /// Where the agent keeps its sessions (`~` allowed).
    pub sessions_dir: Option<String>,
}

impl Detector {
    fn new(name: &str, exe: &[&str], cmdline: &[&str], sessions_dir: Option<&str>) -> Self {
        Self {
            name: name.to_owned(),
            exe: exe.iter().map(|s| (*s).to_owned()).collect(),
            cmdline: cmdline.iter().map(|s| (*s).to_owned()).collect(),
            exclude: Vec::new(),
            sessions_dir: sessions_dir.map(str::to_owned),
        }
    }

    /// Whether a process (name, executable path, arguments) is this agent.
    pub fn matches(&self, name: &str, exe: Option<&Path>, cmd: &[String]) -> bool {
        let norm = |s: &str| s.to_ascii_lowercase().replace('\\', "/");
        let exe_path = exe.map(|p| norm(&p.to_string_lossy())).unwrap_or_default();
        let args: Vec<String> = cmd.iter().map(|a| norm(a)).collect();
        if self
            .exclude
            .iter()
            .map(|x| norm(x))
            .any(|x| exe_path.contains(&x) || args.iter().any(|a| a.contains(&x)))
        {
            return false;
        }
        let stem = name.to_ascii_lowercase();
        let stem = stem.strip_suffix(".exe").unwrap_or(&stem);
        if self.exe.iter().any(|e| e.eq_ignore_ascii_case(stem)) {
            return true;
        }
        self.cmdline
            .iter()
            .map(|p| norm(p))
            .any(|p| args.iter().any(|a| a.contains(&p)))
    }

    pub fn sessions_path(&self) -> Option<PathBuf> {
        let dir = self.sessions_dir.as_deref()?;
        let path = match dir.strip_prefix("~") {
            Some(rest) => home()?.join(rest.trim_start_matches(['/', '\\'])),
            None => PathBuf::from(dir),
        };
        path.is_dir().then_some(path)
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Agents recognized out of the box. Users can add more in the config.
pub fn default_detectors() -> Vec<Detector> {
    let mut claude = Detector::new(
        "Claude Code",
        &["claude"],
        &["@anthropic-ai/claude-code/"],
        Some("~/.claude/projects"),
    );
    // The Claude desktop app is also `claude.exe`/`Claude`.
    claude.exclude = vec![
        "AnthropicClaude".into(),
        "/Claude.app/".into(),
        "/Programs/Claude/".into(),
        "--type=".into(),
    ];
    vec![
        claude,
        Detector::new(
            "opencode",
            &["opencode"],
            &["opencode-ai"],
            Some("~/.local/share/opencode"),
        ),
        Detector::new(
            "Codex",
            &["codex"],
            &["@openai/codex/"],
            Some("~/.codex/sessions"),
        ),
        Detector::new(
            "Gemini CLI",
            &["gemini"],
            &["@google/gemini-cli/"],
            Some("~/.gemini"),
        ),
        Detector::new(
            "Aider",
            &["aider"],
            &["aider-chat", "/bin/aider", "/scripts/aider"],
            None,
        ),
        Detector::new("Cursor Agent", &["cursor-agent"], &["cursor-agent"], None),
        Detector::new(
            "Goose",
            &["goose"],
            &[],
            Some("~/.local/share/goose/sessions"),
        ),
        Detector::new("Amp", &[], &["@sourcegraph/amp/"], None),
        Detector::new("Copilot CLI", &[], &["@github/copilot/"], None),
    ]
}

/// The process filter `mao-sysmon` uses to decide whose cwd to read.
pub fn detail_filter(detectors: Arc<Vec<Detector>>) -> DetailFilter {
    Arc::new(move |name: &str, exe: Option<&Path>, cmd: &[String]| {
        detectors.iter().any(|d| d.matches(name, exe, cmd))
    })
}

/// One running agent session.
#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    pub name: String,
    pub pid: u32,
    pub cwd: Option<PathBuf>,
    /// CPU (% of the machine) and memory of the agent and its child processes.
    pub cpu: f32,
    pub memory: u64,
    pub run_time: u64,
    pub sessions_dir: Option<PathBuf>,
    /// Parent, grandparent… (to find the terminal it runs in).
    pub ancestors: Vec<u32>,
}

/// Agents sharing a working directory (a project).
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub cwd: Option<PathBuf>,
    pub agents: Vec<Agent>,
}

const MAX_DEPTH: usize = 32;

/// Finds the agents in a process list, one per session (the outermost
/// matching process), sorted by project then name.
pub fn detect(procs: &[ProcInfo], detectors: &[Detector]) -> Vec<Agent> {
    let by_pid: HashMap<u32, &ProcInfo> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in procs {
        if let Some(parent) = p.parent {
            children.entry(parent).or_default().push(p.pid);
        }
    }
    let ancestors = |start: u32| {
        let mut out = Vec::new();
        let mut cur = by_pid.get(&start).and_then(|p| p.parent);
        while let Some(pid) = cur {
            if out.len() >= MAX_DEPTH || pid == start || out.contains(&pid) {
                break;
            }
            out.push(pid);
            cur = by_pid.get(&pid).and_then(|p| p.parent);
        }
        out
    };
    let matched: HashMap<u32, usize> = procs
        .iter()
        .filter_map(|p| {
            let d = detectors
                .iter()
                .position(|d| d.matches(&p.name, p.exe.as_deref(), &p.cmd))?;
            Some((p.pid, d))
        })
        .collect();

    let mut agents: Vec<Agent> = Vec::new();
    for (&pid, &d) in &matched {
        let up = ancestors(pid);
        // A child of the same agent (e.g. node spawned by the launcher): skip.
        if up.iter().any(|a| matched.get(a) == Some(&d)) {
            continue;
        }
        let Some(p) = by_pid.get(&pid) else {
            continue;
        };
        // Fold the whole process tree into the session.
        let (mut cpu, mut memory) = (0.0, 0);
        let mut stack = vec![pid];
        let mut seen = 0;
        while let Some(cur) = stack.pop() {
            seen += 1;
            if seen > 4096 {
                break;
            }
            if let Some(q) = by_pid.get(&cur) {
                cpu += q.cpu;
                memory += q.memory;
            }
            if let Some(kids) = children.get(&cur) {
                stack.extend(kids);
            }
        }
        let detector = &detectors[d];
        agents.push(Agent {
            name: detector.name.clone(),
            pid,
            cwd: p.cwd.clone(),
            cpu,
            memory,
            run_time: p.run_time,
            sessions_dir: detector.sessions_path(),
            ancestors: up,
        });
    }
    agents.sort_by(|a, b| {
        a.cwd
            .cmp(&b.cwd)
            .then_with(|| a.name.cmp(&b.name))
            .then(a.pid.cmp(&b.pid))
    });
    agents
}

/// Groups agents by working directory (input sorted by [`detect`]).
pub fn group(agents: &[Agent]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for a in agents {
        match groups.last_mut() {
            Some(g) if g.cwd == a.cwd => g.agents.push(a.clone()),
            _ => groups.push(Group {
                cwd: a.cwd.clone(),
                agents: vec![a.clone()],
            }),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, parent: Option<u32>, name: &str, exe: &str, cmd: &[&str]) -> ProcInfo {
        ProcInfo {
            pid,
            parent,
            name: name.into(),
            exe: Some(PathBuf::from(exe)),
            cmd: cmd.iter().map(|s| (*s).to_owned()).collect(),
            cwd: Some(PathBuf::from(format!("/work/{pid}"))),
            cpu: 1.0,
            memory: 100,
            run_time: 60,
        }
    }

    #[test]
    fn matches_by_exe_and_cmdline() {
        let d = default_detectors();
        let claude = &d[0];
        assert!(claude.matches(
            "claude.exe",
            Some(Path::new(r"C:\Users\m\.local\bin\claude.exe")),
            &[]
        ));
        assert!(claude.matches(
            "node",
            Some(Path::new("/usr/bin/node")),
            &[
                "node".into(),
                "/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js".into()
            ]
        ));
        // The desktop app is not Claude Code.
        assert!(!claude.matches(
            "claude.exe",
            Some(Path::new(
                r"C:\Users\m\AppData\Local\AnthropicClaude\app-1.0\claude.exe"
            )),
            &[]
        ));
        assert!(!claude.matches("node", None, &["server.js".into()]));
        // VS Code's embedded Copilot runtime is not Copilot CLI.
        let copilot = d.iter().find(|d| d.name == "Copilot CLI").unwrap();
        assert!(!copilot.matches(
            "copilot-runtime.exe",
            None,
            &[r"c:\Code\resources\app\node_modules.asar.unpacked\@github\copilot-sdk-win32-x64\copilot-runtime.exe".into()]
        ));
        assert!(copilot.matches(
            "node",
            None,
            &[
                "node".into(),
                "/usr/lib/node_modules/@github/copilot/index.js".into()
            ]
        ));
    }

    #[test]
    fn folds_children_and_finds_ancestors() {
        let procs = vec![
            proc(1, None, "explorer", "/x", &[]),
            proc(10, Some(1), "pwsh", "/pwsh", &[]),
            proc(20, Some(10), "claude", "/bin/claude", &[]),
            // A child claude process and an MCP server below it.
            proc(21, Some(20), "claude", "/bin/claude", &[]),
            proc(22, Some(20), "node", "/node", &["mcp-server.js"]),
            proc(30, Some(10), "opencode", "/bin/opencode", &[]),
        ];
        let agents = detect(&procs, &default_detectors());
        assert_eq!(agents.len(), 2);
        let claude = agents.iter().find(|a| a.pid == 20).unwrap();
        assert_eq!(claude.name, "Claude Code");
        assert!((claude.cpu - 3.0).abs() < 1e-6);
        assert_eq!(claude.memory, 300);
        assert_eq!(claude.ancestors, [10, 1]);
        assert_eq!(group(&agents).len(), 2);
    }

    #[test]
    fn parent_cycles_do_not_hang() {
        let procs = vec![
            proc(5, Some(6), "codex", "/codex", &[]),
            proc(6, Some(5), "x", "/x", &[]),
        ];
        assert_eq!(detect(&procs, &default_detectors()).len(), 1);
    }
}
