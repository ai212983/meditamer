#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
ui_dir="$repo_root/src/firmware/ui/lvgl"

screen_loads="$(rg -n 'lv_screen_load\(' "$ui_dir" || true)"
screen_load_count="$(printf '%s\n' "$screen_loads" | awk 'NF { count += 1 } END { print count + 0 }')"
if [[ "$screen_load_count" -ne 1 || "$screen_loads" != *'/backend.rs:'* ]]; then
  echo "ui-shell ownership: lv_screen_load must appear exactly once, in backend.rs" >&2
  printf '%s\n' "$screen_loads" >&2
  exit 1
fi

for surface in home.rs launcher.rs gesture_test.rs ambient_picker.rs overlay_settings.rs; do
  if rg -n 'super::.*\b(home|launcher|gesture_test|ambient_picker|overlay_settings)\b' "$ui_dir/$surface"; then
    echo "ui-shell ownership: $surface imports a sibling surface" >&2
    exit 1
  fi
done

if ! rg -q 'shell: DefaultShellModel' "$ui_dir/backend.rs"; then
  echo "ui-shell ownership: Backend does not own DefaultShellModel" >&2
  exit 1
fi

if ! rg -q 'catalogue: DefaultCatalogue' "$ui_dir/backend.rs" \
  || ! rg -q 'CatalogueViewKind::Launcher' "$ui_dir/launcher.rs"; then
  echo "ui-shell ownership: launcher must remain a presenter over the shell catalogue" >&2
  exit 1
fi

if rg -n 'launch_diagnostics_callback|open_launcher_callback|home_callback' "$ui_dir"; then
  echo "ui-shell ownership: fixed-destination screen callbacks must not return" >&2
  exit 1
fi

if rg -n '\b(NavIntent|SurfaceRef|DefaultShellModel)\b|lv_screen_load\(' \
  "$repo_root/src/firmware/runtime/serial_task" \
  "$repo_root/src/firmware/runtime/display_task"; then
  echo "ui-shell ownership: runtime and serial control must remain semantic" >&2
  exit 1
fi

echo "ui-shell ownership: pass"
