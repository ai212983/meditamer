#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
ui_dir="$repo_root/products/meditamer/src/firmware/ui/lvgl"
screen_dir="$repo_root/products/meditamer/src/firmware/ui/screen"
widget_dir="$repo_root/products/meditamer/src/firmware/ui/widget"
overlay_dir="$repo_root/products/meditamer/src/firmware/ui/overlay"
serial_dir="$repo_root/products/meditamer/src/firmware/serial"
display_dir="$repo_root/products/meditamer/src/firmware/display"
adapter_dir="$repo_root/platform/ui/render/src/lvgl_adapter"

for dir in "$ui_dir" "$screen_dir" "$widget_dir" "$overlay_dir" "$serial_dir" "$display_dir"; do
  if [[ ! -d "$dir" ]]; then
    echo "ui-shell ownership: configured scan root does not exist: ${dir#"$repo_root"/}" >&2
    exit 1
  fi
done

screen_loads="$(rg -n 'lv_screen_load\(' \
  "$repo_root/platform" "$repo_root/products" "$repo_root/targets" || true)"
screen_load_count="$(printf '%s\n' "$screen_loads" | awk 'NF { count += 1 } END { print count + 0 }')"
if [[ "$screen_load_count" -ne 1 ]] \
  || grep -Ev "^$adapter_dir/raw\.rs:" <<<"$screen_loads" >/dev/null; then
  echo "ui-shell ownership: managed and bootstrap screen loads must stay in the checked adapter" >&2
  printf '%s\n' "$screen_loads" >&2
  exit 1
fi

# Every full-panel, independently navigable screen.
peer_screens=(home launcher gesture_test ambient_view overlay_toggles provider_fixture)
# Lower-level infrastructure may serve several peer screens; it may not reach
# back into any one of them.
infra_files=("$screen_dir/catalogue_presenter.rs" "$widget_dir/carousel.rs")

for surface in "${peer_screens[@]}"; do
  # A surface module may be a single file or a directory (e.g. ambient_view,
  # which also has a pure, non-LVGL model.rs submodule); resolve either.
  if [[ -d "$screen_dir/$surface" ]]; then
    surface_path="$screen_dir/$surface/mod.rs"
  else
    surface_path="$screen_dir/$surface.rs"
  fi
  if [[ ! -f "$surface_path" ]]; then
    echo "ui-shell ownership: configured surface does not exist: ${surface_path#"$repo_root"/}" >&2
    exit 1
  fi
  others="$(printf '%s|' "${peer_screens[@]}" | sed "s/${surface}|//; s/|$//")"
  if rg -n "(super::.*|crate::firmware::ui::screen::.*)\\b(${others})\\b" "$surface_path"; then
    echo "ui-shell ownership: $surface imports a sibling surface" >&2
    exit 1
  fi
done

all_screens="$(printf '%s|' "${peer_screens[@]}" | sed 's/|$//')"
for infra in "${infra_files[@]}"; do
  if [[ ! -f "$infra" ]]; then
    echo "ui-shell ownership: configured infrastructure does not exist: ${infra#"$repo_root"/}" >&2
    exit 1
  fi
  if rg -n "(super::.*|crate::firmware::ui::screen::.*)\\b(${all_screens})\\b" "$infra"; then
    echo "ui-shell ownership: $(basename "$infra") reaches back into a screen surface" >&2
    exit 1
  fi
done

if ! rg -q 'coordinator: MeditamerCoordinator' "$ui_dir/backend.rs"; then
  echo "ui-shell ownership: Backend does not own the UiCoordinator" >&2
  exit 1
fi

if ! rg -q 'catalogue: DefaultCatalogue' "$ui_dir/backend.rs" \
  || ! rg -q 'CatalogueViewKind::Launcher' "$screen_dir/launcher.rs"; then
  echo "ui-shell ownership: launcher must remain a presenter over the shell catalogue" >&2
  exit 1
fi

if rg -n 'launch_diagnostics_callback|open_launcher_callback|home_callback' \
  "$ui_dir" "$screen_dir" "$overlay_dir" "$widget_dir"; then
  echo "ui-shell ownership: fixed-destination screen callbacks must not return" >&2
  exit 1
fi

if rg -n '\b(NavIntent|SurfaceRef|DefaultShellModel)\b|lv_screen_load\(' \
  "$serial_dir" "$display_dir"; then
  echo "ui-shell ownership: runtime and serial control must remain semantic" >&2
  exit 1
fi

echo "ui-shell ownership: pass"
