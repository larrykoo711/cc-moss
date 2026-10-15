//! Which system this is, and how to install the tools cc-moss relies on.

/// What decides the install commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Homebrew.
    MacOs,
    /// Debian, Ubuntu and their derivatives: apt.
    Debian,
    /// Anything else: cc-moss names the tools but runs nothing.
    Other,
}

impl Platform {
    pub fn detect() -> Platform {
        if cfg!(target_os = "macos") {
            return Platform::MacOs;
        }
        std::fs::read_to_string("/etc/os-release")
            .map(|t| Platform::from_os_release(&t))
            .unwrap_or(Platform::Other)
    }

    /// From the text of `/etc/os-release`.
    pub fn from_os_release(text: &str) -> Platform {
        let value = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
                .map(|v| v.trim_matches('"').to_ascii_lowercase())
                .unwrap_or_default()
        };
        let ids = format!("{} {}", value("ID"), value("ID_LIKE"));
        if ids
            .split_whitespace()
            .any(|id| id == "debian" || id == "ubuntu")
        {
            Platform::Debian
        } else {
            Platform::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Tmux,
    Starship,
    Eza,
    Bat,
    Fd,
}

impl Tool {
    /// Name to show and Homebrew formula.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Tmux => "tmux",
            Tool::Starship => "starship",
            Tool::Eza => "eza",
            Tool::Bat => "bat",
            Tool::Fd => "fd",
        }
    }

    /// Any of these on the PATH means installed.
    pub fn commands(self) -> &'static [&'static str] {
        match self {
            Tool::Tmux => &["tmux"],
            Tool::Starship => &["starship"],
            Tool::Eza => &["eza"],
            Tool::Bat => &["bat", "batcat"],
            Tool::Fd => &["fd", "fdfind"],
        }
    }

    /// Needed for cc-moss's own setup; the rest serve optional aliases.
    pub fn is_core(self) -> bool {
        matches!(self, Tool::Tmux | Tool::Starship)
    }

    fn apt_package(self) -> Option<&'static str> {
        match self {
            Tool::Tmux => Some("tmux"),
            Tool::Eza => Some("eza"),
            Tool::Bat => Some("bat"),
            Tool::Fd => Some("fd-find"),
            // Not packaged on older releases; see install_commands.
            Tool::Starship => None,
        }
    }
}

const STARSHIP_INSTALL: &str = "mkdir -p ~/.local/bin && f=$(mktemp) && \
curl -fsSL --proto '=https' --tlsv1.2 -o \"$f\" https://starship.rs/install.sh && \
sh \"$f\" -y -b ~/.local/bin; s=$?; rm -f \"$f\"; exit $s";

/// Shell commands that install `tools`.
pub fn install_commands(platform: Platform, tools: &[Tool]) -> Vec<String> {
    if tools.is_empty() {
        return Vec::new();
    }
    match platform {
        Platform::MacOs => {
            let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
            vec![format!("brew install {}", names.join(" "))]
        }
        Platform::Debian => {
            // Core tools together; optional ones one by one, so a package an
            // older release lacks (eza) does not stop the rest.
            let (core, optional): (Vec<Tool>, Vec<Tool>) = tools.iter().partition(|t| t.is_core());
            let mut groups: Vec<Vec<&str>> = Vec::new();
            let core: Vec<&str> = core.iter().filter_map(|t| t.apt_package()).collect();
            if !core.is_empty() {
                groups.push(core);
            }
            groups.extend(
                optional
                    .iter()
                    .filter_map(|t| t.apt_package())
                    .map(|p| vec![p]),
            );
            let mut cmds: Vec<String> = groups
                .iter()
                .enumerate()
                .map(|(i, pkgs)| {
                    let install = format!("sudo apt-get install -y {}", pkgs.join(" "));
                    // Fresh machines often have no package lists yet.
                    if i == 0 {
                        format!("sudo apt-get update && {install}")
                    } else {
                        install
                    }
                })
                .collect();
            if tools.contains(&Tool::Starship) {
                // The official installer, downloaded first (-f: an HTTP error
                // is a failure, not a script), into ~/.local/bin: no sudo.
                cmds.push(STARSHIP_INSTALL.into());
            }
            cmds
        }
        Platform::Other => Vec::new(),
    }
}

/// Whether any of `commands` is an executable on the PATH.
pub fn on_path(commands: &[&str]) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        commands.iter().any(|c| {
            std::fs::metadata(dir.join(c))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

/// The same commands for root, who has no use for (and often no) sudo.
pub fn without_sudo(commands: Vec<String>) -> Vec<String> {
    commands
        .into_iter()
        .map(|c| c.replace("sudo ", ""))
        .collect()
}
