# shellcheck shell=bash
# ── EXE compat checker (.exe hover) ──────────────────────────────────────
# The Rust binary is copied into /usr/bin by the Dockerfile builder stage.
# Do not reinstall the retired Python source fixture from /ctx over it.
if [[ ! -x /usr/bin/kyth-exe-compat ]]; then
	echo "kyth-exe-compat: native Rust binary missing from image builder" >&2
	exit 1
fi
# mimeapps.list xdg-open interceptor hash-gated: .exe → kyth-exe-compat
if [[ -f /usr/share/applications/kyth-exe-compat.desktop ]]; then
    : # already
else
    cat > /usr/share/applications/kyth-exe-compat.desktop <<'DESKEOF'
[Desktop Entry]
Type=Application
Name=Kyth EXE Compat Check
Exec=/usr/bin/kyth-exe-compat %f
MimeType=application/x-ms-dos-executable;application/x-msdos-program;
NoDisplay=true
DESKEOF
fi
