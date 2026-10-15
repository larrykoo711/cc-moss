use moss_core::platform::{install_commands, Platform, Tool};

#[test]
fn os_release_tells_debian_family_apart() {
    let ubuntu = "NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n";
    let debian = "ID=debian\n";
    let mint = "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n";
    let fedora = "ID=fedora\n";
    assert_eq!(Platform::from_os_release(ubuntu), Platform::Debian);
    assert_eq!(Platform::from_os_release(debian), Platform::Debian);
    assert_eq!(Platform::from_os_release(mint), Platform::Debian);
    assert_eq!(Platform::from_os_release(fedora), Platform::Other);
}

#[test]
fn macos_installs_everything_with_one_brew_command() {
    let cmds = install_commands(Platform::MacOs, &[Tool::Tmux, Tool::Starship, Tool::Bat]);
    assert_eq!(cmds, ["brew install tmux starship bat"]);
}

#[test]
fn debian_uses_apt_and_the_official_starship_script() {
    let cmds = install_commands(Platform::Debian, &[Tool::Tmux, Tool::Fd, Tool::Starship]);
    assert_eq!(
        cmds[0],
        "sudo apt-get update && sudo apt-get install -y tmux"
    );
    // Optional tools one by one: a package missing from older releases (eza)
    // must not take the others down with it.
    assert_eq!(cmds[1], "sudo apt-get install -y fd-find");
    assert!(
        cmds[2].contains("curl -fsSL --proto '=https' --tlsv1.2"),
        "{}",
        cmds[2]
    );
    assert!(
        cmds[2].contains("https://starship.rs/install.sh"),
        "{}",
        cmds[2]
    );
    assert!(
        !cmds[2].contains("| sh"),
        "never pipe a download into sh: {}",
        cmds[2]
    );
    assert_eq!(cmds.len(), 3);
}

#[test]
fn the_first_apt_command_refreshes_the_lists() {
    let cmds = install_commands(Platform::Debian, &[Tool::Eza, Tool::Bat]);
    assert_eq!(
        cmds,
        [
            "sudo apt-get update && sudo apt-get install -y eza",
            "sudo apt-get install -y bat",
        ]
    );
}

#[test]
fn other_systems_get_no_commands() {
    assert!(install_commands(Platform::Other, &[Tool::Tmux]).is_empty());
    assert!(install_commands(Platform::MacOs, &[]).is_empty());
}

#[test]
fn debian_names_count_as_installed() {
    // Debian ships bat as batcat and fd as fdfind.
    assert!(Tool::Bat.commands().contains(&"batcat"));
    assert!(Tool::Fd.commands().contains(&"fdfind"));
}

#[test]
fn root_needs_no_sudo() {
    use moss_core::platform::without_sudo;
    let cmds = install_commands(Platform::Debian, &[Tool::Tmux]);
    assert_eq!(
        without_sudo(cmds),
        ["apt-get update && apt-get install -y tmux"]
    );
}
