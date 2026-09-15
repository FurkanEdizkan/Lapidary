//! `lapidary register` and `lapidary unregister`: the XDG handler that sends `lapidary://` links
//! to `lapidary open` (Phase 4 slice 2 spec §3). Linux only.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The handler's file name, which `xdg-mime` records as the default for [`SCHEME`].
const ENTRY: &str = "lapidary-url.desktop";
const SCHEME: &str = "x-scheme-handler/lapidary";
/// Marks a desktop file as one `register` wrote, so `unregister` never removes somebody else's.
const MARKER: &str = "X-Lapidary-Handler=1";

/// The desktop file. The server and workspace ride in `Exec`: a handler a browser starts does not
/// see the shell's environment, and a link must never be able to choose either.
fn entry(binary: &str, server: &str, workspace: &str) -> Result<String, String> {
    // `env` reads any leading word holding `=` as a variable to set, so an install path with one
    // would run whatever `open` is on the PATH instead of this binary.
    if binary.contains('=') {
        return Err(format!(
            "{binary:?} holds `=`, which `env` would read as a variable rather than as this program. Move lapidary to a path without one, then register again."
        ));
    }
    let exec = [
        "env".to_owned(),
        argument(&format!("LAPIDARY_SERVER={server}"))?,
        argument(&format!("LAPIDARY_WORKSPACE={workspace}"))?,
        argument(binary)?,
        "open".to_owned(),
        "%u".to_owned(),
    ]
    .join(" ");
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=Lapidary link handler\nExec={exec}\n\
         MimeType={SCHEME};\nNoDisplay=true\nTerminal=false\n{MARKER}\n"
    ))
}

/// One `Exec` argument, written as it is, or refused.
///
/// Not quoted, though the Desktop Entry Specification allows quoting: `xdg-open`'s own launcher,
/// the one that runs outside GNOME and KDE, splits `Exec` on spaces and keeps the quotes, so a
/// quoted argument reached `env` as `"LAPIDARY_SERVER=…"` in the slice 2 check. An argument
/// that would need quoting is therefore refused, with a message saying what to change.
fn argument(value: &str) -> Result<String, String> {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._-:=@+,".contains(c);
    if value.is_empty() || !value.chars().all(plain) {
        return Err(format!(
            "{value:?} holds a character a desktop launcher could split or misread (anything but \
             letters, digits and / . _ - : = @ + ,). Use a server address, workspace and install \
             path without spaces or symbols, then register again."
        ));
    }
    Ok(value.to_owned())
}

/// `mimeapps.list` without the default `register` set, every other line as it was.
fn without_default(mimeapps: &str) -> String {
    let ours = format!("{SCHEME}={ENTRY}");
    let mut kept = mimeapps
        .lines()
        .filter(|line| line.trim_end().trim_end_matches(';') != ours)
        .collect::<Vec<_>>()
        .join("\n");
    if mimeapps.ends_with('\n') && !kept.is_empty() {
        kept.push('\n');
    }
    kept
}

/// `$<variable>`, else `$HOME/<fallback>`: where the XDG Base Directory Specification puts it.
fn xdg_dir(variable: &str, fallback: &str) -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os(variable).filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").with_context(|| {
        format!("neither {variable} nor HOME is set, so there is nowhere for the handler; set {variable}")
    })?;
    Ok(PathBuf::from(home).join(fallback))
}

/// Written whole under a temporary name and renamed over, so nothing reads half a file.
fn write_whole(path: &Path, contents: &str) -> Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".part");
    std::fs::write(&temporary, contents)
        .with_context(|| format!("could not write {}", Path::new(&temporary).display()))?;
    std::fs::rename(&temporary, path).with_context(|| format!("could not write {}", path.display()))
}

pub fn register(server: &str, workspace: &Path) -> Result<()> {
    let binary = std::env::current_exe().context(
        "could not find this lapidary binary's own path, so there is nothing to register",
    )?;
    let binary = binary.to_str().with_context(|| {
        format!("{} is not a UTF-8 path, which a desktop file cannot name; move lapidary, then register again", binary.display())
    })?;
    std::fs::create_dir_all(workspace)
        .with_context(|| format!("could not create {}", workspace.display()))?;
    let workspace = workspace
        .canonicalize()
        .with_context(|| format!("could not resolve {}", workspace.display()))?;
    let workspace = workspace.to_str().with_context(|| {
        format!("{} is not a UTF-8 path, which a desktop file cannot name; set LAPIDARY_WORKSPACE to another folder", workspace.display())
    })?;

    let applications = xdg_dir("XDG_DATA_HOME", ".local/share")?.join("applications");
    std::fs::create_dir_all(&applications)
        .with_context(|| format!("could not create {}", applications.display()))?;
    let path = applications.join(ENTRY);
    if let Ok(existing) = std::fs::read_to_string(&path)
        && !existing.lines().any(|line| line == MARKER)
    {
        bail!(
            "{} exists and lapidary register did not write it, so it was left alone. Move it aside, then register again.",
            path.display()
        );
    }
    write_whole(
        &path,
        &entry(binary, server, workspace).map_err(anyhow::Error::msg)?,
    )?;

    let status = Command::new("xdg-mime")
        .args(["default", ENTRY, SCHEME])
        .status()
        .context("could not run xdg-mime to make lapidary the handler for lapidary:// links; install xdg-utils, then register again")?;
    if !status.success() {
        bail!(
            "xdg-mime could not make {} the handler for lapidary:// links ({status}). Check that your mimeapps.list is writable, then register again.",
            path.display()
        );
    }
    println!(
        "Registered {}: lapidary:// links now open parts from {server} in {workspace}. Run `lapidary register` again after changing LAPIDARY_SERVER or LAPIDARY_WORKSPACE.",
        path.display()
    );
    Ok(())
}

pub fn unregister() -> Result<()> {
    let path = xdg_dir("XDG_DATA_HOME", ".local/share")?
        .join("applications")
        .join(ENTRY);
    match std::fs::read_to_string(&path) {
        Ok(existing) if existing.lines().any(|line| line == MARKER) => {
            std::fs::remove_file(&path)
                .with_context(|| format!("could not remove {}", path.display()))?;
            println!("Removed {}.", path.display());
        }
        Ok(_) => println!(
            "{} was not written by lapidary register, so it was left alone.",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("No handler was registered at {}.", path.display());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()));
        }
    }
    let mimeapps = xdg_dir("XDG_CONFIG_HOME", ".config")?.join("mimeapps.list");
    if let Ok(list) = std::fs::read_to_string(&mimeapps) {
        let kept = without_default(&list);
        if kept != list {
            write_whole(&mimeapps, &kept)?;
            println!(
                "Removed the lapidary:// default from {}.",
                mimeapps.display()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handler_runs_this_binary_with_the_server_and_workspace_it_was_registered_with() {
        let file = entry(
            "/home/mira/.cargo/bin/lapidary",
            "http://127.0.0.1:8080",
            "/home/mira/Lapidary/workspace",
        )
        .expect("an entry");
        assert!(file.starts_with("[Desktop Entry]\n"));
        assert!(file.contains(
            "\nExec=env LAPIDARY_SERVER=http://127.0.0.1:8080 LAPIDARY_WORKSPACE=/home/mira/Lapidary/workspace /home/mira/.cargo/bin/lapidary open %u\n"
        ));
        assert!(file.contains("\nMimeType=x-scheme-handler/lapidary;\n"));
        assert!(file.lines().any(|line| line == MARKER));
    }

    /// `xdg-open`'s launcher splits `Exec` on spaces and keeps quotes, so anything a quote would
    /// be needed for is refused rather than written in a form one launcher misreads.
    #[test]
    fn a_handler_argument_a_launcher_could_split_or_misread_is_refused() {
        assert_eq!(
            argument("LAPIDARY_SERVER=https://lapidary.workshop.example:8443").as_deref(),
            Ok("LAPIDARY_SERVER=https://lapidary.workshop.example:8443")
        );
        for value in [
            "/home/mira/Parts and Jigs/lapidary",
            "LAPIDARY_WORKSPACE=$HOME/workspace",
            "/opt/lapidary\"",
            "/opt/lapidary;rm",
            "/opt/lapidary%u",
            "/opt/lapi\ndary",
            "",
        ] {
            assert!(argument(value).is_err(), "{value:?} must be refused");
        }
        assert!(
            entry(
                "/opt/lapidary",
                "http://127.0.0.1:8080",
                "/home/mira/My Parts"
            )
            .is_err(),
            "a workspace with a space registers nothing"
        );
        assert!(
            entry(
                "/opt/tools=2026/lapidary",
                "http://127.0.0.1:8080",
                "/home/mira/Lapidary/workspace"
            )
            .is_err(),
            "an install path `env` would take for an assignment registers nothing"
        );
    }

    #[test]
    fn unregistering_removes_only_the_default_register_set() {
        let list = "[Default Applications]\ntext/plain=org.gnome.TextEditor.desktop\nx-scheme-handler/lapidary=lapidary-url.desktop\nx-scheme-handler/https=google-chrome.desktop\n";
        assert_eq!(
            without_default(list),
            "[Default Applications]\ntext/plain=org.gnome.TextEditor.desktop\nx-scheme-handler/https=google-chrome.desktop\n"
        );
        assert_eq!(
            without_default("x-scheme-handler/lapidary=lapidary-url.desktop;\n"),
            ""
        );
        let others = "x-scheme-handler/lapidary=someone-elses.desktop\n";
        assert_eq!(without_default(others), others);
    }
}
