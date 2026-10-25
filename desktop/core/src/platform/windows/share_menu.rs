//! "Share with Dotlink" in Explorer's right-click menu on files, with one item per paired phone.
//!
//! A static verb under `HKCU\Software\Classes\*\shell\Dotlink` whose items run
//! `dotlink send --notify [--to <id>] "<file>"`. It is rewritten whenever the paired phones
//! change, and removed when there are none. Windows 11 shows it under "Show more options"
//! (Shift+right-click opens that menu directly): its own top-level menu only lists handlers of
//! packaged apps (`IExplorerCommand` with package identity), which needs a signed MSIX.
//!
//! Explorer starts one `dotlink send` per selected file.

use anyhow::Result;
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

const KEY: &str = r"Software\Classes\*\shell\Dotlink";

/// ECF_SEPARATORAFTER: a line under "Every connected phone".
const SEPARATOR_AFTER: u32 = 0x40;

/// `phones` are the paired phones as (id, name).
pub fn set(phones: &[(String, String)]) {
    if let Err(e) = write(phones) {
        tracing::warn!("could not update the Explorer menu: {e:#}");
    }
}

fn write(phones: &[(String, String)]) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    match hkcu.delete_subkey_all(KEY) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    // The id goes on a command line: phones send a UUID, anything else is left out.
    let phones: Vec<_> =
        phones.iter().filter(|(id, _)| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).collect();
    if phones.is_empty() {
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    let exe = exe.display();
    let (root, _) = hkcu.create_subkey(KEY)?;
    root.set_value("MUIVerb", &"Share with Dotlink")?;
    root.set_value("Icon", &format!("\"{exe}\",0"))?;
    // An empty SubCommands: the items are the verbs under `shell` below.
    root.set_value("SubCommands", &"")?;
    let mut items = Vec::new();
    if phones.len() > 1 {
        items.push(("Every connected phone".to_string(), format!("\"{exe}\" send --notify \"%1\""), SEPARATOR_AFTER));
    }
    for (id, name) in phones {
        // `&` marks the access key in menu text; `&&` is a plain one.
        items.push((name.replace('&', "&&"), format!("\"{exe}\" send --notify --to {id} \"%1\""), 0));
    }
    // Explorer orders the items by key name.
    for (i, (label, command, flags)) in items.iter().enumerate() {
        let (item, _) = root.create_subkey(format!(r"shell\{i:02}"))?;
        item.set_value("MUIVerb", label)?;
        // Without it the item disappears when more than 15 files are selected.
        item.set_value("MultiSelectModel", &"Player")?;
        if *flags != 0 {
            item.set_value("CommandFlags", flags)?;
        }
        item.create_subkey("command")?.0.set_value("", command)?;
    }
    Ok(())
}
