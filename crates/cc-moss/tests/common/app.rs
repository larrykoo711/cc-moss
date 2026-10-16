//! Stand-ins for cc-moss.app and the macOS tools that start and stop it.
//! Nothing here is a real app, so nothing can launch; and the stubs never
//! reach the app the developer may have running.

#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub const BUNDLE_ID: &str = "io.github.larrykoo711.cc-moss";

/// An app whose Info.plist says `version`, with an executable that does
/// nothing.
pub fn fake_app(app: &Path, version: &str) {
    let contents = app.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).unwrap();
    std::fs::write(
        contents.join("Info.plist"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n  \
             <key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>\n  \
             <key>CFBundleShortVersionString</key>\n  <string>{version}</string>\n</dict>\n</plist>\n"
        ),
    )
    .unwrap();
    let exe = contents.join("MacOS").join("cc-moss");
    std::fs::write(&exe, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The version a fake app's Info.plist says.
pub fn version(app: &Path) -> Option<String> {
    let text = std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?;
    let (_, rest) = text.split_once("<key>CFBundleShortVersionString</key>")?;
    let (_, rest) = rest.split_once("<string>")?;
    let (version, _) = rest.split_once("</string>")?;
    Some(version.trim().to_string())
}

/// Stub `pgrep`, `osascript` and `open` (and `pkill`/`killall`, which must
/// never be called) into `bin`, logging each call to `<logs>/<name>.log`.
/// With `running`, the app counts as running until `osascript` asks it to
/// quit.
pub fn stubs(bin: &Path, logs: &Path, running: bool) {
    let quit = logs.join("quit");
    let log = |name: &str| format!("echo \"$*\" >> '{}/{name}.log'\n", logs.display());
    let pgrep = if running {
        format!(
            "#!/bin/sh\n{}[ -e '{}' ] && exit 1\nexit 0\n",
            log("pgrep"),
            quit.display()
        )
    } else {
        format!("#!/bin/sh\n{}exit 1\n", log("pgrep"))
    };
    let osascript = format!(
        "#!/bin/sh\n{}touch '{}'\n",
        log("osascript"),
        quit.display()
    );
    std::fs::create_dir_all(bin).unwrap();
    std::fs::create_dir_all(logs).unwrap();
    for (name, script) in [
        ("pgrep", pgrep),
        ("osascript", osascript),
        ("open", format!("#!/bin/sh\n{}", log("open"))),
        ("pkill", format!("#!/bin/sh\n{}", log("pkill"))),
        ("killall", format!("#!/bin/sh\n{}", log("killall"))),
    ] {
        let p = bin.join(name);
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// What a stub logged.
pub fn log(logs: &Path, name: &str) -> String {
    std::fs::read_to_string(logs.join(format!("{name}.log"))).unwrap_or_default()
}

/// Whether a file was left in the app (to tell a replaced app from a kept one).
pub fn mark(app: &Path) {
    std::fs::write(app.join("Contents/mark"), "kept").unwrap();
}

pub fn is_marked(app: &Path) -> bool {
    app.join("Contents/mark").exists()
}
