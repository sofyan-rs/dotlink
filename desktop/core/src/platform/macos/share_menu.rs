//! "Dotlink: <device>" in Finder's right-click menu (Quick Actions, and Services), one item per
//! paired phone or linked PC.
//!
//! Each item is an Automator Quick Action, `~/Library/Services/Dotlink - <name>.workflow`, whose
//! "Run Shell Script" runs `dotlink send [--to <id>] <files>`. They are rewritten whenever the
//! devices change, and removed when there are none. Finder's Share… menu only lists Share
//! extensions (an .appex inside a signed app), which needs a Developer ID.
//!
//! Finder starts one `dotlink send` with every selected file. When it fails, macOS shows the
//! error it printed.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// The workflows Dotlink wrote start with this; anything else in the folder is left alone.
const PREFIX: &str = "Dotlink - ";

/// `phones` are the paired phones and linked PCs as (id, name).
pub fn set(phones: &[(String, String)]) {
    if let Err(e) = write(phones) {
        tracing::warn!("could not update the Finder menu: {e:#}");
    }
}

fn services_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join("Library/Services"))
}

fn write(phones: &[(String, String)]) -> Result<()> {
    let dir = services_dir().context("no home folder")?;
    let mut changed = false;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(PREFIX) && name.ends_with(".workflow") {
                std::fs::remove_dir_all(entry.path())?;
                changed = true;
            }
        }
    }
    // The id goes on a command line: phones send a UUID, PCs a hex id; anything else is left out.
    let phones: Vec<_> =
        phones.iter().filter(|(id, _)| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).collect();
    if !phones.is_empty() {
        let exe = std::env::current_exe()?;
        let exe = exe.to_string_lossy();
        let mut items = Vec::new();
        if phones.len() > 1 {
            items.push(("Every connected device".to_string(), String::new()));
        }
        for (id, name) in phones {
            items.push((name.clone(), format!(" --to {id}")));
        }
        std::fs::create_dir_all(&dir)?;
        for (label, to) in items {
            let script = format!(
                "exe={}\n[ -x \"$exe\" ] || {{ echo \"Dotlink is no longer at $exe\" >&2; exit 1; }}\nexec \"$exe\" send{to} \"$@\"\n",
                shell_quote(&exe)
            );
            write_workflow(&free_path(&dir, &label), &format!("Dotlink: {label}"), &script)?;
        }
        changed = true;
    }
    if changed {
        // Finder reads the Services menu from pbs's cache; without this the items show up late.
        let _ = std::process::Command::new("/System/Library/CoreServices/pbs").arg("-update").spawn();
    }
    Ok(())
}

/// `Dotlink - <label>.workflow`, numbered when two devices have the same name.
fn free_path(dir: &Path, label: &str) -> PathBuf {
    // A file name can't hold `/`, and Finder shows `:` as `/`.
    let label = label.replace(['/', ':'], "-");
    let mut path = dir.join(format!("{PREFIX}{label}.workflow"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{PREFIX}{label} {n}.workflow"));
        n += 1;
    }
    path
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn write_workflow(path: &Path, title: &str, script: &str) -> Result<()> {
    let contents = path.join("Contents");
    std::fs::create_dir_all(&contents)?;
    std::fs::write(contents.join("Info.plist"), info_plist(&xml_escape(title)))?;
    std::fs::write(contents.join("document.wflow"), document(&xml_escape(script)))?;
    Ok(())
}

/// The menu item: files and folders in Finder.
fn info_plist(title: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>NSServices</key>
	<array>
		<dict>
			<key>NSIconName</key>
			<string>NSActionTemplate</string>
			<key>NSMenuItem</key>
			<dict>
				<key>default</key>
				<string>{title}</string>
			</dict>
			<key>NSMessage</key>
			<string>runWorkflowAsService</string>
			<key>NSRequiredContext</key>
			<dict>
				<key>NSApplicationIdentifier</key>
				<string>com.apple.finder</string>
			</dict>
			<key>NSSendFileTypes</key>
			<array>
				<string>public.item</string>
			</array>
		</dict>
	</array>
</dict>
</plist>
"#
    )
}

/// One "Run Shell Script" action, the selected files as arguments (`inputMethod` 1).
fn document(script: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AMApplicationBuild</key>
	<string>534</string>
	<key>AMApplicationVersion</key>
	<string>2.10</string>
	<key>AMDocumentVersion</key>
	<string>2</string>
	<key>actions</key>
	<array>
		<dict>
			<key>action</key>
			<dict>
				<key>AMAccepts</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Optional</key>
					<true/>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>AMActionVersion</key>
				<string>2.0.3</string>
				<key>AMApplication</key>
				<array>
					<string>Automator</string>
				</array>
				<key>AMProvides</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>ActionBundlePath</key>
				<string>/System/Library/Automator/Run Shell Script.action</string>
				<key>ActionName</key>
				<string>Run Shell Script</string>
				<key>ActionParameters</key>
				<dict>
					<key>COMMAND_STRING</key>
					<string>{script}</string>
					<key>CheckedForUserDefaultShell</key>
					<true/>
					<key>inputMethod</key>
					<integer>1</integer>
					<key>shell</key>
					<string>/bin/sh</string>
					<key>source</key>
					<string></string>
				</dict>
				<key>BundleIdentifier</key>
				<string>com.apple.RunShellScript</string>
				<key>CFBundleVersion</key>
				<string>2.0.3</string>
				<key>CanShowSelectedItemsWhenRun</key>
				<false/>
				<key>CanShowWhenRun</key>
				<true/>
				<key>Category</key>
				<array>
					<string>AMCategoryUtilities</string>
				</array>
				<key>Class Name</key>
				<string>RunShellScriptAction</string>
				<key>InputUUID</key>
				<string>5B0E2A44-6C1D-4F57-9E3A-1D2C3B4A5F60</string>
				<key>OutputUUID</key>
				<string>8D7C6B5A-4E3F-4A2B-9C1D-0E9F8A7B6C5D</string>
				<key>UUID</key>
				<string>2F1E0D9C-8B7A-4C6D-8E5F-4A3B2C1D0E9F</string>
				<key>UnlocalizedApplications</key>
				<array>
					<string>Automator</string>
				</array>
				<key>isViewVisible</key>
				<integer>1</integer>
			</dict>
			<key>isViewVisible</key>
			<integer>1</integer>
		</dict>
	</array>
	<key>connectors</key>
	<dict/>
	<key>workflowMetaData</key>
	<dict>
		<key>applicationBundleIDsByPath</key>
		<dict/>
		<key>applicationPaths</key>
		<array/>
		<key>inputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>outputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>presentationMode</key>
		<integer>15</integer>
		<key>processesInput</key>
		<false/>
		<key>serviceApplicationBundleID</key>
		<string>com.apple.finder</string>
		<key>serviceApplicationPath</key>
		<string>/System/Library/CoreServices/Finder.app</string>
		<key>serviceInputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>serviceOutputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>serviceProcessesInput</key>
		<false/>
		<key>systemImageName</key>
		<string>NSActionTemplate</string>
		<key>useAutomaticInputType</key>
		<false/>
		<key>workflowTypeIdentifier</key>
		<string>com.apple.Automator.servicesMenu</string>
	</dict>
</dict>
</plist>
"#
    )
}
