//! Assets for the window: gpui-component's own icons plus the Lucide icons Dotlink uses
//! (embedded one by one, not the whole 1,800-icon set).

use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

gpui_kit_assets::icon_assets!(pub Lucide, [
    Smartphone, Monitor, ScanLine, ArrowDownUp, ListChecks, Settings2, Upload, Files, ClipboardCopy, Cast, Square, Play,
    Copy, FolderOpen, Trash, Usb, Link, Wrench, Bug, Sparkles, RefreshCw, RotateCcw, Download, Check, X, Power, Save,
    ExternalLink, Folder, Laptop, Ellipsis, Plus, Camera, Mic, ArrowUp, File, Pencil, FolderPlus
]);

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        // gpui-kit's own source errors on paths it doesn't know, so ours go first.
        if let Some(bytes) = Lucide.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut paths = gpui_kit_assets::Assets.list(path)?;
        paths.extend(Lucide.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
