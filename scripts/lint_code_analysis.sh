#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

if ! command -v rust-code-analysis-cli >/dev/null 2>&1; then
  cat >&2 <<'EOF'
rust-code-analysis-cli is required for code-metrics linting.
Install with:
  cargo install --locked rust-code-analysis-cli --version 0.0.25
EOF
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  cat >&2 <<'EOF'
jq is required for code-metrics linting.
Install with:
  brew install jq
EOF
  exit 1
fi

top_n="${RCA_TOP_N:-10}"
enforce="${RCA_ENFORCE:-0}"
ratchet="${RCA_RATCHET:-0}"
update_baseline="${RCA_UPDATE_BASELINE:-0}"
baseline_path="${RCA_BASELINE_PATH:-config/rca-baseline.json}"
max_file_sloc="${RCA_MAX_FILE_SLOC:-500}"
warn_file_sloc="${RCA_WARN_FILE_SLOC:-420}"
max_fn_cognitive="${RCA_MAX_FN_COGNITIVE:-40}"
max_fn_cyclomatic="${RCA_MAX_FN_CYCLOMATIC:-32}"
max_fn_nargs="${RCA_MAX_FN_NARGS:-8}"

tmp_metrics="$(mktemp)"
trap 'rm -f "$tmp_metrics"' EXIT

declare -a args=(
  -m
  -p src
  -p tools
  -p packages/sdcard/src
  -p build.rs
  -I '**/*.rs'
  -X '**/target/**'
  -X '**/out/**'
  -X '**/.git/**'
  -O json
)

(
  cd "$repo_root"
  rust-code-analysis-cli "${args[@]}" >"$tmp_metrics"
)

if [[ ! -s "$tmp_metrics" ]]; then
  echo "rust-code-analysis: no analyzable Rust files found"
  exit 0
fi

echo "rust-code-analysis summary"
jq -s '
  def nodes: recurse(.spaces[]?);
  def funcs: nodes | select(.kind == "function");
  {
    files: length,
    functions: ([.[] | funcs] | length),
    p95_fn_cognitive: ([.[] | funcs | (.metrics.cognitive.max // 0)] | sort | if length == 0 then 0 else .[(length * 0.95 | floor)] end),
    p95_fn_cyclomatic: ([.[] | funcs | (.metrics.cyclomatic.max // 0)] | sort | if length == 0 then 0 else .[(length * 0.95 | floor)] end)
  }
' "$tmp_metrics"

echo
echo "top files by SLOC"
jq -r -s --argjson top "$top_n" '
  sort_by(.metrics.loc.sloc // 0)
  | reverse
  | .[:$top]
  | .[]
  | "\((.metrics.loc.sloc // 0) | floor)\t\(.name)"
' "$tmp_metrics"

echo
echo "top functions by cognitive complexity (cognitive, cyclomatic, file::function [start-end])"
jq -r -s --argjson top "$top_n" '
  [ .[] as $f
    | ($f | recurse(.spaces[]?) | select(.kind == "function")
      | {
          file: $f.name,
          name,
          start_line,
          end_line,
          cognitive: (.metrics.cognitive.max // 0),
          cyclomatic: (.metrics.cyclomatic.max // 0)
        })
  ]
  | sort_by(.cognitive)
  | reverse
  | .[:$top]
  | .[]
  | "\(.cognitive)\t\(.cyclomatic)\t\(.file)::\(.name) [\(.start_line)-\(.end_line)]"
' "$tmp_metrics"

echo
echo "top functions by argument count (nargs, cognitive, file::function [start-end])"
jq -r -s --argjson top "$top_n" '
  [ .[] as $f
    | ($f | recurse(.spaces[]?) | select(.kind == "function")
      | {
          file: $f.name,
          name,
          start_line,
          end_line,
          nargs: (.metrics.nargs.total // 0),
          cognitive: (.metrics.cognitive.max // 0)
        })
  ]
  | sort_by(.nargs)
  | reverse
  | .[:$top]
  | .[]
  | "\(.nargs)\t\(.cognitive)\t\(.file)::\(.name) [\(.start_line)-\(.end_line)]"
' "$tmp_metrics"

offenders_json="$(jq -c -s \
  --argjson max_file_sloc "$max_file_sloc" \
  --argjson max_fn_cognitive "$max_fn_cognitive" \
  --argjson max_fn_cyclomatic "$max_fn_cyclomatic" \
  --argjson max_fn_nargs "$max_fn_nargs" '
  def nodes: recurse(.spaces[]?);
  def funcs($file):
    nodes
    | select(.kind == "function")
    | {
        file: $file,
        name,
        start_line,
        end_line,
        cognitive: (.metrics.cognitive.max // 0),
        cyclomatic: (.metrics.cyclomatic.max // 0),
        nargs: (.metrics.nargs.total // 0)
      };
  {
    file_sloc: [ .[] | select((.metrics.loc.sloc // 0) > $max_file_sloc) | {name, sloc: (.metrics.loc.sloc // 0)} ],
    fn_cognitive: [ .[] as $f | ($f | funcs($f.name)) | select(.cognitive > $max_fn_cognitive) ],
    fn_cyclomatic: [ .[] as $f | ($f | funcs($f.name)) | select(.cyclomatic > $max_fn_cyclomatic) ],
    fn_nargs: [ .[] as $f | ($f | funcs($f.name)) | select(.nargs > $max_fn_nargs) ]
  }
' "$tmp_metrics")"

echo
echo "offender counts (thresholds: file_sloc>$max_file_sloc, fn_cognitive>$max_fn_cognitive, fn_cyclomatic>$max_fn_cyclomatic, fn_nargs>$max_fn_nargs)"
echo "$offenders_json" | jq '{
  file_sloc: (.file_sloc | length),
  fn_cognitive: (.fn_cognitive | length),
  fn_cyclomatic: (.fn_cyclomatic | length),
  fn_nargs: (.fn_nargs | length)
}'

warn_file_sloc_json="$(jq -c -s --argjson warn "$warn_file_sloc" --argjson max "$max_file_sloc" '
  [ .[]
    | select((.metrics.loc.sloc // 0) >= $warn and (.metrics.loc.sloc // 0) <= $max)
    | {name, sloc: (.metrics.loc.sloc // 0)}
  ]
' "$tmp_metrics")"
echo
echo "warning counts (thresholds: file_sloc>=$warn_file_sloc and <=$max_file_sloc)"
echo "$warn_file_sloc_json" | jq '{file_sloc_warn: length}'

baseline_json="$(jq -c -s \
  --argjson max_file_sloc "$max_file_sloc" \
  --argjson warn_file_sloc "$warn_file_sloc" \
  --argjson max_fn_cognitive "$max_fn_cognitive" \
  --argjson max_fn_cyclomatic "$max_fn_cyclomatic" \
  --argjson max_fn_nargs "$max_fn_nargs" '
  def nodes: recurse(.spaces[]?);
  def fn_key($file; $name): ($file + "::" + $name);
  def merge_max:
    reduce .[] as $item ({}; .[$item.key] = ((.[$item.key] // 0) | if . > $item.value then . else $item.value end));
  {
    version: 1,
    thresholds: {
      max_file_sloc: $max_file_sloc,
      warn_file_sloc: $warn_file_sloc,
      max_fn_cognitive: $max_fn_cognitive,
      max_fn_cyclomatic: $max_fn_cyclomatic,
      max_fn_nargs: $max_fn_nargs
    },
    offenders: {
      file_sloc: (
        [ .[] | select((.metrics.loc.sloc // 0) > $max_file_sloc) | { key: .name, value: (.metrics.loc.sloc // 0) } ]
        | merge_max
      ),
      fn_cognitive: (
        [ .[] as $f
          | ($f | nodes | select(.kind == "function")
            | select((.metrics.cognitive.max // 0) > $max_fn_cognitive)
            | { key: fn_key($f.name; .name), value: (.metrics.cognitive.max // 0) })
        ]
        | merge_max
      ),
      fn_cyclomatic: (
        [ .[] as $f
          | ($f | nodes | select(.kind == "function")
            | select((.metrics.cyclomatic.max // 0) > $max_fn_cyclomatic)
            | { key: fn_key($f.name; .name), value: (.metrics.cyclomatic.max // 0) })
        ]
        | merge_max
      ),
      fn_nargs: (
        [ .[] as $f
          | ($f | nodes | select(.kind == "function")
            | select((.metrics.nargs.total // 0) > $max_fn_nargs)
            | { key: fn_key($f.name; .name), value: (.metrics.nargs.total // 0) })
        ]
        | merge_max
      )
    }
  }
' "$tmp_metrics")"

if [[ "$update_baseline" == "1" ]]; then
  baseline_file="$repo_root/$baseline_path"
  mkdir -p "$(dirname "$baseline_file")"
  echo "$baseline_json" | jq '.' >"$baseline_file"
  echo
  echo "updated ratchet baseline: $baseline_path"
fi

if [[ "$enforce" == "1" ]]; then
  if [[ "$ratchet" == "1" ]]; then
    baseline_file="$repo_root/$baseline_path"
    if [[ ! -f "$baseline_file" ]]; then
      echo >&2 "ratchet baseline not found: $baseline_path"
      echo >&2 "create/update it with: RCA_UPDATE_BASELINE=1 scripts/lint_code_analysis.sh"
      exit 2
    fi
    ratchet_eval_json="$(jq -n \
      --slurpfile baseline_arr "$baseline_file" \
      --argjson current "$baseline_json" '
      ($baseline_arr[0] // {}) as $baseline
      |
      def eval_cat($name):
        ($baseline.offenders[$name] // {}) as $base
        | ($current.offenders[$name] // {}) as $cur
        | {
            new: (
              [ $cur | to_entries[] | select(($base[.key] // null) == null) | { key, value } ]
            ),
            regressed: (
              [ $cur | to_entries[] | select(($base[.key] // null) != null and (.value > ($base[.key] // 0))) | { key, value, baseline: ($base[.key] // 0) } ]
            )
          };
      {
        file_sloc: eval_cat("file_sloc"),
        fn_cognitive: eval_cat("fn_cognitive"),
        fn_cyclomatic: eval_cat("fn_cyclomatic"),
        fn_nargs: eval_cat("fn_nargs")
      }
    ')"
    echo
    echo "ratchet enforcement summary"
    echo "$ratchet_eval_json" | jq '{
      file_sloc_new: (.file_sloc.new | length),
      file_sloc_regressed: (.file_sloc.regressed | length),
      fn_cognitive_new: (.fn_cognitive.new | length),
      fn_cognitive_regressed: (.fn_cognitive.regressed | length),
      fn_cyclomatic_new: (.fn_cyclomatic.new | length),
      fn_cyclomatic_regressed: (.fn_cyclomatic.regressed | length),
      fn_nargs_new: (.fn_nargs.new | length),
      fn_nargs_regressed: (.fn_nargs.regressed | length)
    }'
    total_ratchet_failures="$(echo "$ratchet_eval_json" | jq '
      (.file_sloc.new | length) + (.file_sloc.regressed | length) +
      (.fn_cognitive.new | length) + (.fn_cognitive.regressed | length) +
      (.fn_cyclomatic.new | length) + (.fn_cyclomatic.regressed | length) +
      (.fn_nargs.new | length) + (.fn_nargs.regressed | length)
    ')"
    if [[ "$total_ratchet_failures" != "0" ]]; then
      echo >&2
      echo >&2 "rust-code-analysis ratchet failed: $total_ratchet_failures new/regressed offenders."
      echo "$ratchet_eval_json" | jq -r '
        [
          {label:"file_sloc new", items:.file_sloc.new},
          {label:"file_sloc regressed", items:.file_sloc.regressed},
          {label:"fn_cognitive new", items:.fn_cognitive.new},
          {label:"fn_cognitive regressed", items:.fn_cognitive.regressed},
          {label:"fn_cyclomatic new", items:.fn_cyclomatic.new},
          {label:"fn_cyclomatic regressed", items:.fn_cyclomatic.regressed},
          {label:"fn_nargs new", items:.fn_nargs.new},
          {label:"fn_nargs regressed", items:.fn_nargs.regressed}
        ]
        | .[]
        | select((.items | length) > 0)
        | .label,
          (.items[] | if has("baseline") then "  \(.key): \(.baseline) -> \(.value)" else "  \(.key): \(.value)" end)
      ' >&2
      exit 2
    fi
  else
    total_offenders="$(echo "$offenders_json" | jq '
      (.file_sloc | length) + (.fn_cognitive | length) + (.fn_cyclomatic | length) + (.fn_nargs | length)
    ')"
    if [[ "$total_offenders" != "0" ]]; then
      echo >&2
      echo >&2 "rust-code-analysis lint failed: $total_offenders offenders above configured thresholds."
      exit 2
    fi
  fi
fi
