//! `jevmarket service`: run the daemon as a per-user service that starts at login and restarts
//! after a crash. launchd on macOS, systemd on Linux.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use anyhow::{Context as _, Result, bail, ensure};
use clap::{Parser, Subcommand};

use crate::config::{ENV_OVERRIDES, Paths, Settings};
use crate::daemon;

const LABEL: &str = "com.coderscantina.jevmarket";
const UNIT: &str = "jevmarket.service";
/// The service sees none of the shell's environment, except these, which move the config and
/// data directories.
const PASSED_ENV: [&str; 2] = ["XDG_CONFIG_HOME", "XDG_DATA_HOME"];

#[derive(Subcommand)]
pub enum Action {
    /// Install the daemon as a service and start it. Run again to change its flags.
    Install {
        /// Flags for `jevmarket daemon`, e.g. `--dry-run --loop 600`.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, value_name = "DAEMON FLAGS")]
        flags: Vec<String>,
    },
    /// Stop the service and remove it.
    Uninstall,
    /// Restart the service, e.g. after upgrading jevmarket.
    Restart,
    /// Show whether the service runs and where it logs.
    Status,
}

/// Parses the install flags exactly as `jevmarket daemon` would.
#[derive(Parser)]
#[command(name = "jevmarket daemon", no_binary_name = true)]
struct DaemonFlags {
    #[command(flatten)]
    args: daemon::Args,
}

pub fn run(paths: &Paths, action: Action) -> Result<()> {
    let manager = Manager::detect()?;
    match action {
        Action::Install { flags } => install(paths, manager, flags),
        Action::Uninstall => manager.uninstall(),
        Action::Restart => manager.restart(),
        Action::Status => manager.status(),
    }
}

fn install(paths: &Paths, manager: Manager, flags: Vec<String>) -> Result<()> {
    let daemon = DaemonFlags::try_parse_from(&flags).unwrap_or_else(|e| e.exit()).args;
    let settings = Settings::load(&paths.config)?;
    let config = std::path::absolute(&paths.config)?;
    let mut argv = vec![executable()?.display().to_string(), "--config".into(), config.display().to_string()];
    argv.push("daemon".into());
    argv.extend(flags);
    let env: Vec<(&str, String)> = PASSED_ENV
        .iter()
        .filter_map(|&var| Some((var, env::var(var).ok()?)))
        .chain([("NO_COLOR", "1".into())])
        .collect();

    manager.install(&argv, &env)?;
    println!("jevmarket runs as a service, console on {}", daemon.console_url());
    for (var, key) in ENV_OVERRIDES {
        if env::var(var).is_ok_and(|v| !v.trim().is_empty()) {
            println!("{var} is set in this shell, but the service does not see it: `jevmarket config set {key} ...`");
        }
    }
    if !(settings.dry_run || daemon.dry_run) {
        println!("Live mode: after every start the loop waits paused until you resume it in the console.");
    }
    Ok(())
}

/// The running binary. Homebrew runs it from a versioned Cellar directory that the next upgrade
/// deletes, so a service points at the stable link in the prefix's `bin/` instead.
fn executable() -> Result<PathBuf> {
    let exe = env::current_exe().context("locating the jevmarket binary")?;
    let link = exe
        .ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "Cellar"))
        .and_then(Path::parent)
        .zip(exe.file_name())
        .map(|(prefix, name)| prefix.join("bin").join(name));
    Ok(link.filter(|link| link.exists()).unwrap_or(exe))
}

#[derive(Clone, Copy)]
enum Manager {
    Launchd,
    Systemd,
}

impl Manager {
    fn detect() -> Result<Self> {
        match env::consts::OS {
            "macos" => Ok(Self::Launchd),
            "linux" => Ok(Self::Systemd),
            os => bail!(
                "`service` supports macOS (launchd) and Linux (systemd), not {os}. Run `jevmarket daemon` under your own service manager"
            ),
        }
    }

    fn file(self) -> Result<PathBuf> {
        let home = etcetera::home_dir().context("could not determine the home directory")?;
        Ok(match self {
            Self::Launchd => home.join("Library/LaunchAgents").join(format!("{LABEL}.plist")),
            Self::Systemd => env::var_os("XDG_CONFIG_HOME")
                .map_or_else(|| home.join(".config"), PathBuf::from)
                .join("systemd/user")
                .join(UNIT),
        })
    }

    fn log_file() -> Result<PathBuf> {
        Ok(etcetera::home_dir().context("could not determine the home directory")?.join("Library/Logs/jevmarket.log"))
    }

    fn install(self, argv: &[String], env: &[(&str, String)]) -> Result<()> {
        let file = self.file()?;
        fs::create_dir_all(file.parent().expect("a file in a directory"))?;
        match self {
            Self::Launchd => {
                fs::write(&file, launchd_plist(argv, env, &Self::log_file()?))?;
                // Replaces a loaded older version; fails harmlessly when there is none.
                let _ = Command::new("launchctl").args(["bootout", &launchd_target()?]).output();
                exec("launchctl", &["bootstrap", &format!("gui/{}", uid()?), &file.display().to_string()])
            }
            Self::Systemd => {
                fs::write(&file, systemd_unit(argv, env))?;
                systemctl(&["daemon-reload"])?;
                systemctl(&["enable", UNIT])?;
                systemctl(&["restart", UNIT])?;
                if linger() == Some(false) {
                    println!(
                        "The service stops when you log out. To keep it running: sudo loginctl enable-linger {}",
                        env::var("USER").unwrap_or_else(|_| "$USER".into())
                    );
                }
                Ok(())
            }
        }
    }

    fn uninstall(self) -> Result<()> {
        let file = self.file()?;
        ensure!(file.exists(), "the service is not installed");
        match self {
            Self::Launchd => {
                let _ = Command::new("launchctl").args(["bootout", &launchd_target()?]).output();
            }
            Self::Systemd => systemctl(&["disable", "--now", UNIT])?,
        }
        fs::remove_file(&file)?;
        if let Self::Systemd = self {
            systemctl(&["daemon-reload"])?;
        }
        println!("Removed the service. Config and database stay where they are.");
        Ok(())
    }

    fn restart(self) -> Result<()> {
        ensure!(self.file()?.exists(), "the service is not installed: `jevmarket service install`");
        match self {
            Self::Launchd => exec("launchctl", &["kickstart", "-k", &launchd_target()?]),
            Self::Systemd => systemctl(&["restart", UNIT]),
        }
    }

    fn status(self) -> Result<()> {
        let file = self.file()?;
        if !file.exists() {
            println!("not installed");
            return Ok(());
        }
        println!("service file  {}", file.display());
        match self {
            Self::Launchd => {
                let out = Command::new("launchctl").args(["print", &launchd_target()?]).output()?;
                if !out.status.success() {
                    println!("not loaded: `jevmarket service install` loads it again");
                    return Ok(());
                }
                // `launchctl print` dumps dozens of lines; these answer "is it up".
                for line in String::from_utf8_lossy(&out.stdout).lines().map(str::trim) {
                    if ["state = ", "pid = ", "last exit code = "].iter().any(|key| line.starts_with(key)) {
                        println!("{line}");
                    }
                }
                println!("logs          tail -f {}", Self::log_file()?.display());
            }
            Self::Systemd => {
                // Exits non-zero for a stopped unit, which is an answer, not an error.
                let _ = Command::new("systemctl").args(["--user", "status", UNIT, "--no-pager", "--lines=0"]).status();
                println!("logs          journalctl --user -u jevmarket -f");
            }
        }
        Ok(())
    }
}

fn exec(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program).args(args).status().with_context(|| format!("running {program}"))?;
    ensure!(status.success(), "`{program} {}` failed ({status})", args.join(" "));
    Ok(())
}

fn systemctl(args: &[&str]) -> Result<()> {
    exec("systemctl", &[&["--user"], args].concat())
}

/// `Some(false)` only when logind says so; unknown counts as fine.
fn linger() -> Option<bool> {
    let user = env::var("USER").ok()?;
    let out = Command::new("loginctl").args(["show-user", &user, "--property=Linger", "--value"]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim() == "yes")
}

fn uid() -> Result<String> {
    let out = Command::new("id").arg("-u").output().context("running id -u")?;
    Ok(String::from_utf8(out.stdout)?.trim().to_owned())
}

fn launchd_target() -> Result<String> {
    Ok(format!("gui/{}/{LABEL}", uid()?))
}

fn launchd_plist(argv: &[String], env: &[(&str, String)], log: &Path) -> String {
    let xml = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let args: String = argv.iter().map(|a| format!("\n    <string>{}</string>", xml(a))).collect();
    let vars: String =
        env.iter().map(|(k, v)| format!("\n    <key>{k}</key>\n    <string>{}</string>", xml(v))).collect();
    let log = xml(&log.display().to_string());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>{args}
  </array>
  <key>EnvironmentVariables</key>
  <dict>{vars}
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <!-- Restart after a crash; a clean exit (SIGTERM) stays stopped. -->
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>ThrottleInterval</key>
  <integer>10</integer>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#
    )
}

fn systemd_unit(argv: &[String], env: &[(&str, String)]) -> String {
    let exec_start: Vec<String> = argv.iter().map(|a| systemd_quote(a)).collect();
    let vars: String =
        env.iter().map(|(k, v)| format!("Environment={}\n", systemd_quote(&format!("{k}={v}")))).collect();
    format!(
        "[Unit]\nDescription=jevmarket daemon and web console\n\n\
         [Service]\nExecStart={}\n{vars}Restart=on-failure\nRestartSec=10\n\n\
         [Install]\nWantedBy=default.target\n",
        exec_start.join(" ")
    )
}

/// One double-quoted systemd word: `\` and `"` escaped, `%` and `$` doubled so systemd does not
/// expand them.
fn systemd_quote(s: &str) -> String {
    let escaped = s.replace('\\', r"\\").replace('"', "\\\"").replace('%', "%%").replace('$', "$$");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_flags_parse_like_the_daemon() {
        let flags = ["--dry-run", "--loop", "600", "--base-path", "jev/"].map(String::from);
        let daemon = DaemonFlags::try_parse_from(flags).unwrap().args;
        assert!(daemon.dry_run);
        assert_eq!(daemon.console_url(), "http://127.0.0.1:8787/jev/");
        assert!(DaemonFlags::try_parse_from(["--nope"]).is_err());
    }

    #[test]
    fn systemd_words_survive_quoting() {
        assert_eq!(systemd_quote(r#"/a b/"x"\%$"#), r#""/a b/\"x\"\\%%$$""#);
    }

    #[test]
    fn plist_escapes_xml() {
        let plist = launchd_plist(&["/a&b".into()], &[("K", "<v>".into())], Path::new("/l"));
        assert!(plist.contains("<string>/a&amp;b</string>"));
        assert!(plist.contains("<string>&lt;v&gt;</string>"));
    }
}
