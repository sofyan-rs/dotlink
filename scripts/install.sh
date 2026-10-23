#!/usr/bin/env bash
# Builds and installs the Dotlink desktop app for the current user, on any Linux desktop.
#
#   scripts/install.sh              build, install to ~/.local/bin + app menu, start it at login
#   scripts/install.sh --camera     also create "Dotlink Camera" (v4l2loopback, needs sudo)
#   scripts/install.sh --headless   no window or tray: just the service + CLI (`dotlink daemon`)
#   scripts/install.sh --yes        install missing build packages without asking
#
# Nothing here needs root except --camera and the optional fixes it prints. The app's Setup
# screen can make the same fixes later.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$HOME/.local/bin"
camera=0 headless=0 yes=0
for arg in "$@"; do
    case $arg in
        --camera) camera=1 ;;
        --headless) headless=1 ;;
        --yes|-y) yes=1 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done
ok() { printf '  \033[32m✓\033[0m %s\n' "$*"; }
todo() { printf '  \033[33m!\033[0m %s\n' "$*"; }

# ---------- package manager ----------
if command -v dnf >/dev/null; then pm=dnf; install="sudo dnf install"
elif command -v apt-get >/dev/null; then pm=apt; install="sudo apt install"
elif command -v pacman >/dev/null; then pm=pacman; install="sudo pacman -S --needed"
elif command -v zypper >/dev/null; then pm=zypper; install="sudo zypper install"
else pm=other; install="install"
fi

# Package that provides a command, per distro.
pkg() {
    case "$pm:$1" in
        *:wl-copy) echo wl-clipboard ;;
        *:notify-send) case $pm in apt) echo libnotify-bin ;; pacman) echo libnotify ;; *) echo libnotify ;; esac ;;
        *:pw-cat) case $pm in apt) echo pipewire-bin ;; *) echo pipewire ;; esac ;;
        dnf:ffmpeg) echo "ffmpeg (RPM Fusion) or ffmpeg-free" ;;
        *:ffmpeg) echo ffmpeg ;;
        *:cargo) echo "rustup (https://rustup.rs)" ;;
        *) echo "$1" ;;
    esac
}

echo "==> checking tools ($pm)"
missing=()
for cmd in cargo ffmpeg pw-cat notify-send; do
    if command -v "$cmd" >/dev/null; then ok "$cmd"; else todo "$cmd missing"; missing+=("$(pkg "$cmd")"); fi
done
# wl-clipboard is only needed on Wayland compositors with data-control (not GNOME).
if [ -n "${WAYLAND_DISPLAY:-}" ] && ! command -v wl-copy >/dev/null; then
    todo "wl-copy missing (used on Hyprland, Sway, niri, KDE…)"; missing+=("$(pkg wl-copy)")
fi
if [ ${#missing[@]} -gt 0 ]; then
    echo "    install: $install ${missing[*]}"
    command -v cargo >/dev/null || exit 1
fi

# The window (GPUI) links against these at build time.
if [ $headless = 0 ] && command -v pkg-config >/dev/null; then
    gui_missing=()
    for lib in xkbcommon xkbcommon-x11 wayland-client xcb fontconfig freetype2; do
        pkg-config --exists "$lib" || gui_missing+=("$lib")
    done
    if [ ${#gui_missing[@]} -gt 0 ]; then
        todo "missing build libraries for the window: ${gui_missing[*]}"
        # Only needed to build; the installed app uses the runtime libraries every desktop has.
        pkgs=()
        for lib in "${gui_missing[@]}"; do
            case "$pm:$lib" in
                dnf:freetype2) pkgs+=(freetype-devel) ;;
                zypper:freetype2) pkgs+=(freetype2-devel) ;;
                dnf:xcb|zypper:xcb) pkgs+=(libxcb-devel) ;;
                dnf:wayland-client|zypper:wayland-client) pkgs+=(wayland-devel) ;;
                dnf:*|zypper:*) pkgs+=("lib$lib-devel") ;;
                apt:freetype2) pkgs+=(libfreetype-dev) ;;
                apt:xcb) pkgs+=(libxcb1-dev) ;;
                apt:wayland-client) pkgs+=(libwayland-dev) ;;
                apt:fontconfig) pkgs+=(libfontconfig-dev) ;;
                apt:*) pkgs+=("lib$lib-dev") ;;
                pacman:wayland-client) pkgs+=(wayland) ;;
                pacman:xcb) pkgs+=(libxcb) ;;
                pacman:*) pkgs+=("${lib/#xkbcommon/libxkbcommon}") ;;
            esac
        done
        # zypper names it fontconfig-devel, not libfontconfig-devel.
        pkgs=("${pkgs[@]/libfontconfig-devel/fontconfig-devel}")
        if [ $pm = other ]; then
            echo "    install the development packages for: ${gui_missing[*]}"
            echo "    (or build without the window: scripts/install.sh --headless)"
            exit 1
        fi
        echo "    needs: $install ${pkgs[*]}"
        answer=y
        if [ $yes = 0 ]; then
            read -r -p "    Install them now? [Y/n] " answer || answer=n
        fi
        case "${answer:-y}" in
            [Yy]*) $install "${pkgs[@]}" || exit 1 ;;
            *) echo "    (or build without the window: scripts/install.sh --headless)"; exit 1 ;;
        esac
        ok "build libraries installed"
    fi
fi

echo "==> building"
features=()
[ $headless = 1 ] && features=(--no-default-features)
cargo build --release --manifest-path "$root/desktop/Cargo.toml" -p dotlink "${features[@]}"
install -Dm755 "$root/desktop/target/release/dotlink" "$bin/dotlink"
ok "installed $bin/dotlink"
case ":$PATH:" in *":$bin:"*) ;; *) todo "$bin is not in PATH; add it in your shell profile" ;; esac

if [ $headless = 0 ]; then
    apps="$HOME/.local/share/applications"
    icons="$HOME/.local/share/icons/hicolor/scalable/apps"
    install -Dm644 "$root/desktop/app/assets/dotlink.svg" "$icons/dotlink.svg"
    mkdir -p "$apps"
    # Named after the Wayland app id, so GNOME and KDE match the window to it (icon, name).
    sed "s|^Exec=dotlink|Exec=$bin/dotlink|" "$root/desktop/app/assets/dev.dotlink.Dotlink.desktop" >"$apps/dev.dotlink.Dotlink.desktop"
    rm -f "$apps/dotlink.desktop" # older installs
    command -v update-desktop-database >/dev/null && update-desktop-database "$apps" 2>/dev/null || true
    ok "added Dotlink to the app menu"
fi

echo "==> autostart"
desktop="${XDG_CURRENT_DESKTOP:-unknown}"
autostart="$HOME/.config/autostart/dotlink.desktop"
if [ $headless = 1 ]; then start="dotlink daemon"; else start="dotlink --background"; fi
mkdir -p "$(dirname "$autostart")"
cat >"$autostart" <<EOF
[Desktop Entry]
Type=Application
Name=Dotlink
Comment=Link your Android phone
Exec=$bin/$start
Icon=dotlink
X-GNOME-Autostart-enabled=true
NoDisplay=true
EOF
ok "wrote $autostart (GNOME, KDE, XFCE, Cinnamon, MATE, LXQt, Budgie start it at login)"
case "${desktop,,}" in
    *hyprland*) todo "Hyprland ignores XDG autostart. Add:  exec-once = $start   (Lua: hl.exec_cmd(\"$start\"))" ;;
    *sway*|*river*|*niri*|*labwc*|*wayfire*) todo "$desktop may ignore XDG autostart. Add '$start' to its startup commands." ;;
    *i3*|*bspwm*|*awesome*|*qtile*) todo "Add '$start &' to your window manager's startup." ;;
esac

echo "==> remote control (uinput)"
if [ -w /dev/uinput ]; then
    ok "/dev/uinput is writable"
else
    todo "/dev/uinput is not writable; remote control needs it. Fix (once):"
    echo "      echo 'KERNEL==\"uinput\", TAG+=\"uaccess\"' | sudo tee /etc/udev/rules.d/70-dotlink-uinput.rules"
    echo "      sudo modprobe uinput && sudo udevadm control --reload && sudo udevadm trigger"
fi

echo "==> firewall (port 47390/tcp, mDNS 5353/udp)"
if command -v firewall-cmd >/dev/null && firewall-cmd --state >/dev/null 2>&1; then
    if firewall-cmd --query-port=47390/tcp >/dev/null 2>&1 || firewall-cmd --list-ports 2>/dev/null | grep -q "1025-65535/tcp"; then
        ok "firewalld allows 47390/tcp"
    else
        todo "firewalld blocks it. Fix: sudo firewall-cmd --permanent --add-port=47390/tcp --add-service=mdns && sudo firewall-cmd --reload"
    fi
elif command -v ufw >/dev/null && sudo -n ufw status 2>/dev/null | grep -q "Status: active"; then
    todo "ufw is active. Fix: sudo ufw allow 47390/tcp && sudo ufw allow 5353/udp"
else
    ok "no active firewall found"
fi

echo "==> camera"
if grep -qx "Dotlink Camera" /sys/class/video4linux/*/name 2>/dev/null; then
    ok "Dotlink Camera exists"
elif [ $camera = 1 ]; then
    if ! modinfo v4l2loopback >/dev/null 2>&1; then
        case $pm in
            dnf) todo "needs v4l2loopback: sudo dnf install v4l2loopback (RPM Fusion)" ;;
            apt) todo "needs v4l2loopback: sudo apt install v4l2loopback-dkms v4l2loopback-utils" ;;
            pacman) todo "needs v4l2loopback: sudo pacman -S v4l2loopback-dkms v4l2loopback-utils" ;;
            *) todo "needs the v4l2loopback kernel module" ;;
        esac
        exit 1
    fi
    sudo modprobe v4l2loopback
    sudo v4l2loopback-ctl add -n "Dotlink Camera" -x 1
    # Recreate it at every boot.
    sudo tee /etc/systemd/system/dotlink-camera.service >/dev/null <<'EOF'
[Unit]
Description=Dotlink Camera (v4l2loopback device)
After=systemd-modules-load.service

[Service]
Type=oneshot
ExecStartPre=/sbin/modprobe v4l2loopback
ExecStart=/bin/sh -c 'grep -qx "Dotlink Camera" /sys/class/video4linux/*/name 2>/dev/null || v4l2loopback-ctl add -n "Dotlink Camera" -x 1'
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
EOF
    sudo systemctl daemon-reload
    sudo systemctl enable dotlink-camera.service
    ok "created Dotlink Camera (kept across reboots)"
else
    todo "no Dotlink Camera yet; run with --camera to create it (needs sudo)"
fi

if [ $headless = 1 ]; then
    echo "==> done. Start it now with 'dotlink daemon &' (or log out and in), then: dotlink pair"
else
    echo "==> done. Open Dotlink from the app menu (or run 'dotlink'), then pair your phone."
fi
