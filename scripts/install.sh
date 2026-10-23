#!/usr/bin/env bash
# Builds and installs the Dotlink daemon for the current user, on any Linux desktop.
#
#   scripts/install.sh            build, install to ~/.local/bin, start it with your desktop
#   scripts/install.sh --camera   also create "Dotlink Camera" (v4l2loopback, needs sudo)
#
# Nothing here needs root except --camera and the optional fixes it prints.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$HOME/.local/bin"
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

echo "==> building"
cargo build --release --manifest-path "$root/daemon/Cargo.toml"
install -Dm755 "$root/daemon/target/release/dotlink" "$bin/dotlink"
ok "installed $bin/dotlink"
case ":$PATH:" in *":$bin:"*) ;; *) todo "$bin is not in PATH; add it in your shell profile" ;; esac

echo "==> autostart"
desktop="${XDG_CURRENT_DESKTOP:-unknown}"
autostart="$HOME/.config/autostart/dotlink.desktop"
mkdir -p "$(dirname "$autostart")"
cat >"$autostart" <<EOF
[Desktop Entry]
Type=Application
Name=Dotlink
Comment=Link your Android phone
Exec=$bin/dotlink daemon
Icon=phone
X-GNOME-Autostart-enabled=true
NoDisplay=true
EOF
ok "wrote $autostart (GNOME, KDE, XFCE, Cinnamon, MATE, LXQt, Budgie start it at login)"
case "${desktop,,}" in
    *hyprland*) todo "Hyprland ignores XDG autostart. Add:  exec-once = dotlink daemon   (Lua: hl.exec_cmd(\"dotlink daemon\"))" ;;
    *sway*|*river*|*niri*|*labwc*|*wayfire*) todo "$desktop may ignore XDG autostart. Add 'dotlink daemon' to its startup commands." ;;
    *i3*|*bspwm*|*awesome*|*qtile*) todo "Add 'dotlink daemon &' to your window manager's startup." ;;
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
elif [ "${1:-}" = "--camera" ]; then
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

echo "==> done. Start it now with 'dotlink daemon &' (or log out and in), then: dotlink pair"
