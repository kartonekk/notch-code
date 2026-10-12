//! Starts Notch Code when the user signs in to Windows.
//!
//! Uses a per-user Task Scheduler logon task rather than the Run registry key:
//! Run entries are held back by Windows' startup-app delay, the task starts
//! right at sign-in. The task also sets normal process priority (a scheduled
//! task otherwise runs below normal) and no time limit (the default is 72 h).

use std::path::Path;
use std::process::{Command, Stdio};

use crate::paths;

const TASK: &str = "Notch Code";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Registers (or removes) the logon task for the running exe. Called on every
/// launch, so the task follows the exe if it was moved.
pub fn apply(enabled: bool) {
    let result = if enabled { register() } else { unregister() };
    match result {
        Ok(()) => paths::log(format!("autostart {}", if enabled { "on" } else { "off" })),
        Err(e) => paths::log(format!("autostart: {e}")),
    }
}

fn schtasks(args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("schtasks.exe");
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn register() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(d), Ok(u)) => format!("{d}\\{u}"),
        (_, Ok(u)) => u,
        _ => return Err("unknown user name".into()),
    };
    // schtasks reads the definition from a UTF-16 file
    let xml_path = paths::data_dir().join("autostart-task.xml");
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(task_xml(&exe, &user).encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(&xml_path, bytes).map_err(|e| e.to_string())?;
    let result = schtasks(&["/Create", "/TN", TASK, "/XML", &xml_path.to_string_lossy(), "/F"]);
    let _ = std::fs::remove_file(&xml_path);
    result
}

fn unregister() -> Result<(), String> {
    if schtasks(&["/Query", "/TN", TASK]).is_err() {
        return Ok(()); // not registered
    }
    schtasks(&["/Delete", "/TN", TASK, "/F"])
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn task_xml(exe: &Path, user: &str) -> String {
    let exe = xml_escape(&exe.to_string_lossy());
    let user = xml_escape(user);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts Notch Code when you sign in. Turn off from the Notch Code tray menu.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
    </Exec>
  </Actions>
</Task>
"#
    )
}
