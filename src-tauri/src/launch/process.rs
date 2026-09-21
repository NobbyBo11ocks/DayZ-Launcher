//! Spawning the game through the BattlEye launcher, exactly like the official
//! launcher does (D-010): `DayZ_BE.exe` from the game folder with the game folder
//! as working directory. Steam must be running; the game loads `steam_api64.dll`.

use std::path::Path;
use std::process::{Child, Command};

use serde::Serialize;

use crate::error::{AppError, AppResult};

use super::display_command_line;

pub const BE_EXE: &str = "DayZ_BE.exe";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Launched {
    pub pid: u32,
    pub exe: String,
    pub command_line: String,
}

pub fn spawn(game_dir: &Path, args: &[String]) -> AppResult<(Child, Launched)> {
    let exe = game_dir.join(BE_EXE);
    if !exe.is_file() {
        return Err(AppError::Internal(format!("{} is missing; verify the game files in Steam", exe.display())));
    }
    let child = Command::new(&exe)
        .args(args)
        .current_dir(game_dir)
        .spawn()
        .map_err(|e| AppError::io(&exe, e))?;
    let launched = Launched {
        pid: child.id(),
        exe: exe.to_string_lossy().into_owned(),
        command_line: display_command_line(&exe.to_string_lossy(), args),
    };
    Ok((child, launched))
}
