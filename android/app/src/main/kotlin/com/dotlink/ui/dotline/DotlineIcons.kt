/**
 * Dotline line icons, drawn with Lucide (https://lucide.dev, ISC licence): 24 dp, 1.5 dp stroke,
 * round caps and joins.
 *
 * The path data is copied from lucide-static 1.59.0 (circles, rects and lines turned into paths).
 * Draw them with `Icon(DotlineIcons.Search, contentDescription)`; the colour comes from `tint`
 * (usually `Dotline.colors.ink` or `inkFaint`). Each comment names the Lucide icon used.
 *
 * To add an icon: pick one on lucide.dev, copy its SVG path data and add one line here.
 */
package com.dotlink.ui.dotline

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

object DotlineIcons {
    /** Back (top bars). Lucide: chevron-left. */
    val Back: ImageVector by lazy { lineIcon("Back", "M15 18l-6-6 6-6") }

    /** Show password. Lucide: eye. */
    val Eye: ImageVector by lazy {
        lineIcon(
            "Eye",
            "M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0M9 12a3 3 0 1 0 6 0a3 3 0 1 0 -6 0z",
        )
    }

    /** Hide password. Lucide: eye-off. */
    val EyeOff: ImageVector by lazy {
        lineIcon(
            "EyeOff",
            "M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49M14.084 14.158a3 3 0 0 1-4.242-4.242M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143M2 2l20 20",
        )
    }

    /** Lucide: check. */
    val Check: ImageVector by lazy { lineIcon("Check", "M20 6 9 17l-5-5") }

    /** ⋮ actions. Lucide: ellipsis-vertical. */
    val More: ImageVector by lazy {
        lineIcon(
            "More",
            "M11 12a1 1 0 1 0 2 0a1 1 0 1 0 -2 0zM11 5a1 1 0 1 0 2 0a1 1 0 1 0 -2 0zM11 19a1 1 0 1 0 2 0a1 1 0 1 0 -2 0z",
        )
    }

    /** Lucide: pencil. */
    val Edit: ImageVector by lazy {
        lineIcon(
            "Edit",
            "M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497zM15 5l4 4",
        )
    }

    /** Lucide: code. */
    val Code: ImageVector by lazy { lineIcon("Code", "M16 18l6-6-6-6M8 6l-6 6 6 6") }

    /** Lucide: link. */
    val Link: ImageVector by lazy {
        lineIcon(
            "Link",
            "M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71",
        )
    }

    /** Lucide: image. */
    val Image: ImageVector by lazy {
        lineIcon(
            "Image",
            "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-14a2 2 0 0 1 2 -2zM7 9a2 2 0 1 0 4 0a2 2 0 1 0 -4 0zM21 15l-3.086-3.086a2 2 0 0 0-2.828 0L6 21",
        )
    }

    /** Lucide: list-checks. */
    val Checklist: ImageVector by lazy { lineIcon("Checklist", "M13 5h8M13 12h8M13 19h8M3 17l2 2 4-4M3 7l2 2 4-4") }

    /** Lucide: list. */
    val BulletList: ImageVector by lazy { lineIcon("BulletList", "M3 5h.01M3 12h.01M3 19h.01M8 5h13M8 12h13M8 19h13") }

    /** Lucide: quote. */
    val Quote: ImageVector by lazy {
        lineIcon(
            "Quote",
            "M16 3a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2 1 1 0 0 1 1 1v1a2 2 0 0 1-2 2 1 1 0 0 0-1 1v2a1 1 0 0 0 1 1 6 6 0 0 0 6-6V5a2 2 0 0 0-2-2zM5 3a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2 1 1 0 0 1 1 1v1a2 2 0 0 1-2 2 1 1 0 0 0-1 1v2a1 1 0 0 0 1 1 6 6 0 0 0 6-6V5a2 2 0 0 0-2-2z",
        )
    }

    /** Lucide: table. */
    val Table: ImageVector by lazy {
        lineIcon(
            "Table",
            "M12 3v18M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-14a2 2 0 0 1 2 -2zM3 9h18M3 15h18",
        )
    }

    /** Move to folder. Lucide: folder-input. */
    val MoveTo: ImageVector by lazy {
        lineIcon(
            "MoveTo",
            "M2 9V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H20a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-1M2 13h10M9 16l3-3-3-3",
        )
    }

    /** Lucide: pin. */
    val Pin: ImageVector by lazy {
        lineIcon(
            "Pin",
            "M12 17v5M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z",
        )
    }

    /** Lucide: copy. */
    val Copy: ImageVector by lazy {
        lineIcon(
            "Copy",
            "M10 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1 -2 2h-10a2 2 0 0 1 -2 -2v-10a2 2 0 0 1 2 -2zM4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2",
        )
    }

    /** Lucide: share. */
    val Share: ImageVector by lazy { lineIcon("Share", "M12 2v13M16 6l-4-4-4 4M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8") }

    /** Lucide: lock. */
    val Lock: ImageVector by lazy {
        lineIcon(
            "Lock",
            "M5 11h14a2 2 0 0 1 2 2v7a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-7a2 2 0 0 1 2 -2zM7 11V7a5 5 0 0 1 10 0v4",
        )
    }

    /** Lucide: trash-2. */
    val Trash: ImageVector by lazy {
        lineIcon(
            "Trash",
            "M10 11v6M14 11v6M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2",
        )
    }

    /** Lucide: list-ordered. */
    val NumberedList: ImageVector by lazy {
        lineIcon(
            "NumberedList",
            "M11 5h10M11 12h10M11 19h10M4 4h1v5M4 9h2M6.5 20H3.4c0-1 2.6-1.925 2.6-3.5a1.5 1.5 0 0 0-2.6-1.02",
        )
    }

    /** Horizontal rule. Lucide: minus. */
    val Rule: ImageVector by lazy { lineIcon("Rule", "M5 12h14") }

    /** Lucide: chevron-down. */
    val ChevronDown: ImageVector by lazy { lineIcon("ChevronDown", "M6 9l6 6 6-6") }

    /** Lucide: chevron-right. */
    val ChevronRight: ImageVector by lazy { lineIcon("ChevronRight", "M9 18l6-6-6-6") }

    /** Lucide: plus. */
    val Plus: ImageVector by lazy { lineIcon("Plus", "M5 12h14M12 5v14") }

    /** Lucide: folder. */
    val Folder: ImageVector by lazy {
        lineIcon(
            "Folder",
            "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z",
        )
    }

    /** Lucide: search. */
    val Search: ImageVector by lazy { lineIcon("Search", "M21 21l-4.34-4.34M3 11a8 8 0 1 0 16 0a8 8 0 1 0 -16 0z") }

    /** SYNC tab. Lucide: git-branch. */
    val GitBranch: ImageVector by lazy {
        lineIcon(
            "GitBranch",
            "M15 6a9 9 0 0 0-9 9V3M15 6a3 3 0 1 0 6 0a3 3 0 1 0 -6 0zM3 18a3 3 0 1 0 6 0a3 3 0 1 0 -6 0z",
        )
    }

    /** VAULTS tab. Lucide: sliders-horizontal. */
    val Sliders: ImageVector by lazy { lineIcon("Sliders", "M10 5H3M12 19H3M14 3v4M16 17v4M21 12h-9M21 19h-5M21 5h-7M8 10v4M8 12H3") }

    /** Settings tab. Lucide: settings-2. */
    val Settings: ImageVector by lazy {
        lineIcon("Settings", "M14 17H5M19 7h-9M14 17a3 3 0 1 0 6 0a3 3 0 1 0 -6 0zM4 7a3 3 0 1 0 6 0a3 3 0 1 0 -6 0z")
    }

    /** Lucide: file-plus. */
    val NewNote: ImageVector by lazy {
        lineIcon(
            "NewNote",
            "M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2zM14 2v5a1 1 0 0 0 1 1h5M9 15h6M12 18v-6",
        )
    }

    /** Lucide: folder-plus. */
    val NewFolder: ImageVector by lazy {
        lineIcon(
            "NewFolder",
            "M12 10v6M9 13h6M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z",
        )
    }

    /** Sync now. Lucide: refresh-cw. */
    val Sync: ImageVector by lazy {
        lineIcon(
            "Sync",
            "M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8M21 3v5h-5M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16M8 16H3v5",
        )
    }

    /** A note file. Lucide: file-text. */
    val Note: ImageVector by lazy {
        lineIcon(
            "Note",
            "M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2zM14 2v5a1 1 0 0 0 1 1h5M10 9H8M16 13H8M16 17H8",
        )
    }

    /** Lucide: x. */
    val Close: ImageVector by lazy { lineIcon("Close", "M18 6 6 18M6 6l12 12") }

    /** Lucide: arrow-right. */
    val ArrowRight: ImageVector by lazy { lineIcon("ArrowRight", "M5 12h14M12 5l7 7-7 7") }

    /** Warning card. Lucide: triangle-alert. */
    val Warning: ImageVector by lazy {
        lineIcon(
            "Warning",
            "M21.73 18l-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3M12 9v4M12 17h.01",
        )
    }

    /** A vault. Lucide: layers. */
    val Vault: ImageVector by lazy {
        lineIcon(
            "Vault",
            "M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83zM2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17",
        )
    }

    /** Remove a vault. Lucide: square-minus. */
    val RemoveVault: ImageVector by lazy {
        lineIcon(
            "RemoveVault",
            "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-14a2 2 0 0 1 2 -2zM8 12h8",
        )
    }

    /** Shell. Lucide: terminal. */
    val Terminal: ImageVector by lazy { lineIcon("Terminal", "M4 17l6-6-6-6M12 19h8") }

    /** Phone camera. Lucide: camera. */
    val Camera: ImageVector by lazy { lineIcon("Camera", "M13.997 4a2 2 0 0 1 1.76 1.05l.486.9A2 2 0 0 0 18.003 7H20a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2h1.997a2 2 0 0 0 1.759-1.048l.489-.904A2 2 0 0 1 10.004 4zM9 13a3 3 0 1 0 6 0a3 3 0 1 0 -6 0z") }

    /** Phone microphone. Lucide: mic. */
    val Mic: ImageVector by lazy { lineIcon("Mic", "M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3zM19 10v2a7 7 0 0 1-14 0v-2M12 19v3") }

    /** Clipboard. Lucide: clipboard. */
    val Clipboard: ImageVector by lazy { lineIcon("Clipboard", "M9 2h6a1 1 0 0 1 1 1v2a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1zM16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2") }

    /** Touchpad / remote. Lucide: mouse. */
    val Mouse: ImageVector by lazy { lineIcon("Mouse", "M5 9a7 7 0 0 1 14 0v6a7 7 0 0 1-14 0zM12 6v4") }

    /** Keyboard. Lucide: keyboard. */
    val Keyboard: ImageVector by lazy { lineIcon("Keyboard", "M4 4h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zM6 8h.01M10 8h.01M14 8h.01M18 8h.01M8 12h.01M12 12h.01M16 12h.01M7 16h10") }

    /** Send to the PC. Lucide: upload. */
    val Upload: ImageVector by lazy { lineIcon("Upload", "M12 15V3M7 8l5-5 5 5M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4") }

    /** Received from the PC. Lucide: download. */
    val Download: ImageVector by lazy { lineIcon("Download", "M12 3v12M7 10l5 5 5-5M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4") }

    /** The paired PC. Lucide: monitor. */
    val Monitor: ImageVector by lazy { lineIcon("Monitor", "M4 3h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM8 21h8M12 17v4") }

    /** Scan a QR code. Lucide: scan-line. */
    val Scan: ImageVector by lazy { lineIcon("Scan", "M3 7V5a2 2 0 0 1 2-2h2M17 3h2a2 2 0 0 1 2 2v2M21 17v2a2 2 0 0 1-2 2h-2M7 21H5a2 2 0 0 1-2-2v-2M7 12h10") }

    /** Rotate the picture. Lucide: rotate-cw. */
    val Rotate: ImageVector by lazy { lineIcon("Rotate", "M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8M21 3v5h-5") }

    /** Mirror the picture. Lucide: flip-vertical-2 (since Lucide 1.0 the left/right mirror has this name). */
    val FlipHorizontal: ImageVector by lazy {
        lineIcon("FlipHorizontal", "M20.288 16.703A1 1 0 0 0 22 16V8a1 1 0 0 0-1.712-.703l-3.99 3.991a1 1 0 0 0-.001 1.424zM3.712 16.703A1 1 0 0 1 2 16V8a1 1 0 0 1 1.712-.703l3.99 3.991a1 1 0 0 1 .001 1.424zM12 2v2M12 8v2M12 14v2M12 20v2")
    }

    /** Camera torch. Lucide: flashlight. */
    val Flashlight: ImageVector by lazy {
        lineIcon("Flashlight", "M17 2a1 1 0 0 1 1 1v4a3 3 0 0 1-.6 1.8l-.6.8A4 4 0 0 0 16 12v8a2 2 0 0 1-2 2h-4a2 2 0 0 1-2-2v-8a4 4 0 0 0-.8-2.4l-.6-.8A3 3 0 0 1 6 7V3a1 1 0 0 1 1-1zM6 6h12M12 13v1")
    }

    /** Camera zoom. Lucide: zoom-in. */
    val ZoomIn: ImageVector by lazy { lineIcon("ZoomIn", "M21 21l-4.34-4.34M3 11a8 8 0 1 0 16 0a8 8 0 1 0 -16 0zM11 8v6M8 11h6") }

    /** Front / back camera. Lucide: switch-camera. */
    val SwitchCamera: ImageVector by lazy { lineIcon("SwitchCamera", "M11 19H4a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h5M13 5h7a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2h-5M9 12a3 3 0 1 0 6 0a3 3 0 1 0 -6 0zM18 22l-3-3 3-3M6 2l3 3-3 3") }

    /** Check for updates. Lucide: refresh-cw. */
    val Refresh: ImageVector by lazy {
        lineIcon("Refresh", "M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8M21 3v5h-5M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16M8 16H3v5")
    }

    /** Report a bug. Lucide: bug. */
    val Bug: ImageVector by lazy {
        lineIcon("Bug", "M12 20v-9M14 7a4 4 0 0 1 4 4v3a6 6 0 0 1-12 0v-3a4 4 0 0 1 4-4zM14.12 3.88 16 2M21 21a4 4 0 0 0-3.81-4M21 5a4 4 0 0 1-3.55 3.97M22 13h-4M3 21a4 4 0 0 1 3.81-4M3 5a4 4 0 0 0 3.55 3.97M6 13H2M8 2l1.88 1.88M9 7.13V6a3 3 0 1 1 6 0v1.13")
    }

    /** About the app. Lucide: info. */
    val Info: ImageVector by lazy { lineIcon("Info", "M2 12a10 10 0 1 0 20 0a10 10 0 1 0 -20 0zM12 16v-4M12 8h.01") }

    /** A website. Lucide: globe. */
    val Globe: ImageVector by lazy {
        lineIcon("Globe", "M2 12a10 10 0 1 0 20 0a10 10 0 1 0 -20 0zM12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20M2 12h20")
    }
}

/**
 * Builds a stroked 24×24 icon from SVG path data. The stroke is black here; `Icon` tints it
 * with the colour you pass. `by lazy` above means each icon is built once, on first use.
 */
private fun lineIcon(
    name: String,
    pathData: String,
): ImageVector =
    ImageVector
        .Builder(name = name, defaultWidth = 24.dp, defaultHeight = 24.dp, viewportWidth = 24f, viewportHeight = 24f)
        .addPath(
            pathData = addPathNodes(pathData),
            fill = null,
            stroke = SolidColor(Color.Black),
            strokeLineWidth = 1.5f,
            strokeLineCap = StrokeCap.Round,
            strokeLineJoin = StrokeJoin.Round,
        ).build()
