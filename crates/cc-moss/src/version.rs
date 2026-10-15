//! `cc-moss --version`: the mark drawn in braille, the name and the slogan.
//! Piped or redirected, it is the plain `cc-moss <version>` scripts expect.

use std::io::IsTerminal;

/// The ∞ mark rasterised onto 36×16 braille dots.
const ART: [&str; 4] = [
    "⣠⣾⡿⠛⠻⢿⣷⣤⡀⢀⣤⣾⡿⠟⠛⢿⣷⣄",
    "⣿⡏    ⠉⢿⣷⣾⡿⠉    ⢹⣿",
    "⣿⣇    ⣀⣾⡿⢿⣷⣀    ⣸⣿",
    "⠙⢿⣷⣤⣴⣾⡿⠛⠁⠈⠛⢿⣷⣦⣤⣾⡿⠋",
];

const BRAND: &str = "\x1b[1;38;2;255;90;60m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub fn print() {
    let version = env!("CARGO_PKG_VERSION");
    let stdout = std::io::stdout();
    if stdout.is_terminal() {
        let color = std::env::var_os("NO_COLOR").is_none();
        print!("{}", banner(version, color));
    } else {
        println!("cc-moss {version}");
    }
}

/// After MOSS, whose mission in *The Wandering Earth* is to keep civilization going.
pub fn slogan() -> &'static str {
    tr!(
        "Civilizations carry on. So do conversations.",
        "文明会延续，对话也是。"
    )
}

fn banner(version: &str, color: bool) -> String {
    let paint = |code: &str, text: &str| {
        if color {
            format!("{code}{text}{RESET}")
        } else {
            text.to_string()
        }
    };
    let side = [
        format!("{}  {}", paint(BOLD, "cc-moss"), paint(DIM, version)),
        slogan().to_string(),
        paint(DIM, "github.com/larrykoo711/cc-moss"),
        String::new(),
    ];
    let mut out = String::from("\n");
    for (art, text) in ART.iter().zip(&side) {
        let line = format!("  {}    {text}", paint(BRAND, art));
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn the_mark_is_a_neat_block() {
        let width = super::ART[0].width();
        assert!(super::ART.iter().all(|l| l.width() == width));
    }

    #[test]
    fn banner_has_the_name_version_and_slogan() {
        let plain = super::banner("9.9.9", false);
        assert!(plain.contains("cc-moss  9.9.9"), "{plain}");
        assert!(plain.contains("So do conversations."), "{plain}");
        assert_eq!(plain.trim().lines().count(), 4, "{plain}");
        assert!(!plain.contains('\x1b'), "no colour when asked not to");
        crate::i18n::set_for_test(crate::i18n::Lang::Zh);
        let zh = super::banner("9.9.9", true);
        crate::i18n::set_for_test(crate::i18n::Lang::En);
        assert!(
            zh.contains("文明会延续，对话也是。") && zh.contains('\x1b'),
            "{zh}"
        );
    }
}
