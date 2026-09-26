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
    /// `-cpuCount`, `-maxMem`, `-maxVRAM` made for this PC, less any the extra
    /// arguments set themselves (D-267).
    pub perf_args: Vec<String>,
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
    v.extend(spec.perf_args.iter().cloned());
    // The join's own keys come from the join: `-connect`, `-port`, `-mod`, `-password`
    // or `-name` in the extra arguments or a launch profile came after ours, and DayZ
    // could take the later one and land the player somewhere else (D-265).
    v.extend(
        split_extra(&spec.extra_args)
            .into_iter()
            .filter(|a| !overrides_join(a)),
    );
    v
}

/// An extra argument that would override something the join itself sets.
fn overrides_join(arg: &str) -> bool {
    let key = arg
        .split('=')
        .next()
        .unwrap_or("")
        .trim_start_matches('-')
        .to_ascii_lowercase();
    matches!(
        key.as_str(),
        "connect" | "port" | "mod" | "password" | "name"
    )
}

/// Splits `a "b c" d` into `[a, b c, d]`; a quote nothing closes is ignored. It used to
/// open a quote that ran to the end of the line, so `-nosplash "-profiles=D:\P -skipintro`
/// handed the game `-skipintro` inside its profile path (D-256).
pub fn split_extra(s: &str) -> Vec<String> {
    // With an odd count, the last quote is the one nothing closes.
    let unmatched = if s.matches('"').count() % 2 == 1 {
        s.rfind('"')
    } else {
        None
    };
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for (i, ch) in s.char_indices() {
        match ch {
            '"' if Some(i) == unmatched => {}
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

/// Windows-style rendering for logs and the UI: arguments containing spaces are
/// quoted, and the server password is masked. The real argument still carries it
/// (the game needs it, D-010), but this string is shown in the join dialog and
/// copied into support reports, where a screenshot would leak it (D-160).
pub fn display_command_line(exe: &str, args: &[String]) -> String {
    let mut s = String::new();
    s.push('"');
    s.push_str(exe);
    s.push('"');
    for a in args {
        s.push(' ');
        let a = match a.strip_prefix("-password=") {
            Some("") | None => a.as_str(),
            Some(_) => "-password=********",
        };
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
            perf_args: Vec::new(),
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
    fn display_masks_the_password() {
        let args = vec![
            "-connect=1.2.3.4".to_string(),
            "-password=hunter2".to_string(),
        ];
        let line = display_command_line("DayZ_BE.exe", &args);
        assert!(line.ends_with("-password=********"), "{line}");
        assert!(!line.contains("hunter2"));
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
    fn extras_cannot_override_the_join() {
        // A profile's extras naming another server, port, mod set, password or name
        // are dropped; everything else passes through (D-265).
        let spec = LaunchSpec {
            ip: "1.2.3.4".into(),
            game_port: 2302,
            extra_args: r#"-connect=9.9.9.9 -PORT=2402 -mod=@X -password=x -name=y -cpuCount=8 -profiles="D:\P""#.into(),
            ..Default::default()
        };
        let args = build_args(&spec);
        assert!(args.contains(&"-connect=1.2.3.4".to_string()));
        assert!(args.contains(&"-port=2302".to_string()));
        assert!(!args
            .iter()
            .any(|a| a.contains("9.9.9.9") || a.contains("2402") || a == "-mod=@X"));
        assert!(!args
            .iter()
            .any(|a| a.starts_with("-password=") || a.starts_with("-name=")));
        assert!(args.ends_with(&["-cpuCount=8".to_string(), r"-profiles=D:\P".to_string()]));
    }

    #[test]
    fn performance_arguments_sit_between_the_flags_and_the_extras() {
        let spec = LaunchSpec {
            ip: "1.2.3.4".into(),
            game_port: 2302,
            no_pause: true,
            perf_args: vec!["-cpuCount=32".into(), "-maxMem=30626".into()],
            extra_args: "-maxVRAM=4096".into(),
            ..Default::default()
        };
        assert_eq!(
            &build_args(&spec)[7..],
            &["-noPause", "-cpuCount=32", "-maxMem=30626", "-maxVRAM=4096"]
        );
    }

    #[test]
    fn split_extra_handles_quotes() {
        assert_eq!(split_extra(r#"a "b c" d"#), vec!["a", "b c", "d"]);
        assert_eq!(split_extra("   "), Vec::<String>::new());
        // A quote nothing closes is ignored instead of swallowing the rest (D-256).
        assert_eq!(
            split_extra(r#"-nosplash "-profiles=D:\P -skipintro"#),
            vec!["-nosplash", r"-profiles=D:\P", "-skipintro"]
        );
        assert_eq!(split_extra(r#"a "b c" "d e"#), vec!["a", "b c", "d", "e"]);
    }
}
