//! Command-line construction, reproducing the official launcher exactly (docs/02 §5):
//!
//! ```text
//! DayZ_BE.exe 0 1 1 -exe DayZ_x64.exe "-mod=<abs>;<abs>" -connect=<ip> -port=<gamePort> [-password=…] -name=<name> …
//! ```
//!
//! Evidence: `%LOCALAPPDATA%\DayZ Launcher\Logs\Launcher.log` `GameExecutor` lines and
//! the game's RPT (`"-mod=G:\…\!Workshop\@CF;G:\…\!Workshop\@Dabs Framework;…"`).

use std::path::PathBuf;

/// The fixed BattlEye launcher prefix; meaning of `0 1 1` is undocumented, kept verbatim (D-010).
pub const BE_PREFIX: [&str; 5] = ["0", "1", "1", "-exe", "DayZ_x64.exe"];

#[derive(Debug, Clone, Default)]
pub struct LaunchSpec {
    /// Absolute junction paths in server-reported order.
    pub mod_paths: Vec<PathBuf>,
    pub ip: String,
    pub game_port: u16,
    pub password: Option<String>,
    pub profile_name: Option<String>,
    pub skip_intro: bool,
    pub no_splash: bool,
    pub no_pause: bool,
    /// Raw extra arguments from settings, split on whitespace (quotes respected).
    pub extra_args: String,
}

pub fn build_args(spec: &LaunchSpec) -> Vec<String> {
    let mut v: Vec<String> = BE_PREFIX.iter().map(|s| s.to_string()).collect();
    if !spec.mod_paths.is_empty() {
        let joined = spec
            .mod_paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(";");
        v.push(format!("-mod={joined}"));
    }
    v.push(format!("-connect={}", spec.ip));
    v.push(format!("-port={}", spec.game_port));
    if let Some(pw) = spec.password.as_deref().filter(|p| !p.is_empty()) {
        v.push(format!("-password={pw}"));
    }
    if let Some(name) = spec
        .profile_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        v.push(format!("-name={name}"));
    }
    if spec.skip_intro {
        v.push("-skipintro".into());
    }
    if spec.no_splash {
        v.push("-nosplash".into());
    }
    if spec.no_pause {
        v.push("-noPause".into());
    }
    v.extend(split_extra(&spec.extra_args));
    v
}

/// Splits `a "b c" d` into `[a, b c, d]`; a lone quote is ignored.
pub fn split_extra(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in s.chars() {
        match ch {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Windows-style rendering for logs and the UI: arguments containing spaces are quoted.
pub fn display_command_line(exe: &str, args: &[String]) -> String {
    let mut s = String::new();
    s.push('"');
    s.push_str(exe);
    s.push('"');
    for a in args {
        s.push(' ');
        if a.contains(' ') {
            s.push('"');
            s.push_str(a);
            s.push('"');
        } else {
            s.push_str(a);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_official_launcher_form() {
        let spec = LaunchSpec {
            mod_paths: vec![
                PathBuf::from(r"G:\SteamLibrary\steamapps\common\DayZ\!Workshop\@CF"),
                PathBuf::from(r"G:\SteamLibrary\steamapps\common\DayZ\!Workshop\@Dabs Framework"),
            ],
            ip: "51.81.8.81".into(),
            game_port: 2402,
            password: None,
            profile_name: Some("Survivor".into()),
            skip_intro: true,
            no_splash: true,
            no_pause: false,
            extra_args: String::new(),
        };
        let args = build_args(&spec);
        assert_eq!(&args[..5], &["0", "1", "1", "-exe", "DayZ_x64.exe"]);
        assert_eq!(
            args[5],
            r"-mod=G:\SteamLibrary\steamapps\common\DayZ\!Workshop\@CF;G:\SteamLibrary\steamapps\common\DayZ\!Workshop\@Dabs Framework"
        );
        assert_eq!(
            &args[6..],
            &[
                "-connect=51.81.8.81",
                "-port=2402",
                "-name=Survivor",
                "-skipintro",
                "-nosplash"
            ]
        );
        let line =
            display_command_line(r"G:\SteamLibrary\steamapps\common\DayZ\DayZ_BE.exe", &args);
        assert!(line.starts_with(r#""G:\SteamLibrary\steamapps\common\DayZ\DayZ_BE.exe" 0 1 1 -exe DayZ_x64.exe "-mod=G:\"#));
        assert!(line.contains(r#"@Dabs Framework" -connect=51.81.8.81 -port=2402"#));
    }

    #[test]
    fn vanilla_password_and_extras() {
        let spec = LaunchSpec {
            ip: "1.2.3.4".into(),
            game_port: 2302,
            password: Some("hunter2".into()),
            profile_name: Some("  ".into()),
            extra_args: r#"-cpuCount=8 -profiles="D:\My Profiles""#.into(),
            ..Default::default()
        };
        let args = build_args(&spec);
        assert!(
            !args.iter().any(|a| a.starts_with("-mod=")),
            "no -mod for vanilla"
        );
        assert!(
            !args.iter().any(|a| a.starts_with("-name=")),
            "blank name is dropped"
        );
        assert_eq!(
            &args[5..],
            &[
                "-connect=1.2.3.4",
                "-port=2302",
                "-password=hunter2",
                "-cpuCount=8",
                r"-profiles=D:\My Profiles"
            ]
        );
    }

    #[test]
    fn split_extra_handles_quotes() {
        assert_eq!(split_extra(r#"a "b c" d"#), vec!["a", "b c", "d"]);
        assert_eq!(split_extra("   "), Vec::<String>::new());
    }
}
