#!/usr/bin/env bash

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/common.sh"

"$SCRIPT_DIR/rebuild-payload.sh"

if ! domain_exists || [[ "$(domain_state)" != "running" ]]; then
    printf 'Payload attached. Start Windows and use the desktop shortcut "Wreath aus Linux aktualisieren".\n'
    exit 0
fi

REQUEST="$(python - <<'PY'
import json
command = (
    "$d=(Get-Volume -FileSystemLabel 'WREATH_TEST').DriveLetter;"
    "$args='-NoProfile -ExecutionPolicy Bypass -File \"' + $d + ':\\Install-Wreath.ps1\" -UpdateOnly';"
    "$action=New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $args -WorkingDirectory ($d + ':\\');"
    "$principal=New-ScheduledTaskPrincipal -UserId ($env:COMPUTERNAME + '\\Wreath') -LogonType Interactive -RunLevel Highest;"
    "Register-ScheduledTask -TaskName 'Wreath Linux Update' -Action $action -Principal $principal -Force | Out-Null;"
    "Start-ScheduledTask -TaskName 'Wreath Linux Update'"
)
print(json.dumps({
    "execute": "guest-exec",
    "arguments": {
        "path": "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
        "arg": ["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", command],
        "capture-output": True,
    },
}))
PY
)"

RESULT="$(box_virsh qemu-agent-command "$DOMAIN_NAME" "$REQUEST" 2>/dev/null || true)"
PID="$(python -c 'import json,sys; print(json.load(sys.stdin).get("return", {}).get("pid", ""))' <<<"$RESULT" 2>/dev/null || true)"
if [[ -n "$PID" ]]; then
    printf 'Wreath update handed to the logged-in Windows user (guest-agent PID %s).\n' "$PID"
else
    printf 'Payload attached. Guest agent is not ready; use "Wreath aus Linux aktualisieren" on the Windows desktop.\n'
fi
