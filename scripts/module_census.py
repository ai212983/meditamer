#!/usr/bin/env python3
"""Whole-module import census for src/.

Resolves every `use` path in every file -- grouped braces expanded, `super::`
chains resolved against the file's own module path -- into a module-to-module
dependency graph between the top-level modules under `firmware/`.

Greps for known module names cannot do this: `use crate::firmware::{a, b::{c}}`
hides its members from a line-oriented search, which is how four separate
first-party dependencies were missed while planning ADR-0015.
"""
import pathlib
import re
import sys
from collections import defaultdict

SRC = pathlib.Path('src')


def module_path(f: pathlib.Path):
    """src/firmware/net/runtime.rs -> ('firmware','net','runtime')"""
    parts = list(f.relative_to(SRC).parts)
    parts[-1] = parts[-1][:-3]                     # drop .rs
    if parts[-1] in ('mod', 'lib', 'main'):
        parts.pop()
    return tuple(parts)


def strip_comments(s: str) -> str:
    s = re.sub(r'//.*', '', s)
    return re.sub(r'/\*.*?\*/', '', s, flags=re.S)


def split_top(body: str):
    """Split a brace group on top-level commas."""
    out, depth, cur = [], 0, ''
    for ch in body:
        if ch == '{':
            depth += 1
        elif ch == '}':
            depth -= 1
        if ch == ',' and depth == 0:
            out.append(cur.strip()); cur = ''
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def expand(prefix: str, item: str):
    """Expand `a::{b, c::{d}}` into full leaf paths."""
    item = item.strip()
    if not item:
        return []
    if '{' in item:
        head, rest = item.split('{', 1)
        rest = rest.rsplit('}', 1)[0]
        base = (prefix + head).strip()
        return [p for sub in split_top(rest) for p in expand(base, sub)]
    return [(prefix + item).strip()]


def imports(text: str):
    """Every leaf `use` path in the file."""
    out = []
    for m in re.finditer(r'\buse\s+', text):
        i = m.end()
        depth, j = 0, i
        while j < len(text):
            c = text[j]
            if c == '{':
                depth += 1
            elif c == '}':
                depth -= 1
            elif c == ';' and depth == 0:
                break
            j += 1
        out += expand('', text[i:j].replace('\n', ' '))
    return out


def resolve(path: str, mod: tuple):
    """Resolve a use-path to an absolute module tuple, or None if external."""
    path = re.sub(r'\s+as\s+\w+$', '', path.strip()).replace(' ', '')
    if path.startswith('crate::'):
        return tuple(path[len('crate::'):].split('::'))
    if path.startswith('self::'):
        return mod + tuple(path[len('self::'):].split('::'))
    if path.startswith('super::'):
        base, rest = mod, path
        while rest.startswith('super::'):
            if not base:
                return None
            base = base[:-1]
            rest = rest[len('super::'):]
        return base + tuple(rest.split('::'))
    return None                                    # external crate


REAL = {p.stem for p in SRC.glob('firmware/*.rs') if p.stem != 'mod'} | \
       {p.name for p in SRC.glob('firmware/*') if p.is_dir()}


def top(abs_mod):
    """firmware::net::runtime::x -> 'net'  (top-level module under firmware)"""
    if not abs_mod or abs_mod[0] != 'firmware' or len(abs_mod) < 2:
        return None
    name = abs_mod[1]
    return name if name in REAL else None


edges = defaultdict(lambda: defaultdict(set))      # src -> dst -> {symbols}
for f in sorted(SRC.rglob('*.rs')):
    mod = module_path(f)
    src_top = top(mod)
    if src_top is None:
        continue
    text = strip_comments(f.read_text())
    # Inline fully-qualified paths used in code bodies, not just `use` items.
    # This is how `crate::firmware::ble::phase1s_ownership()` hid from every
    # earlier search: it is never imported, only called at its full path.
    inline = [m.group(0) for m in re.finditer(r'\b(?:crate|super(?:::super)*)::[A-Za-z_][A-Za-z0-9_:]*', text)]
    for p in imports(text) + inline:
        r = resolve(p, mod)
        dst_top = top(r) if r else None
        if dst_top and dst_top != src_top:
            edges[src_top][dst_top].add('::'.join(r[2:]) or dst_top)

mods = sorted(set(edges) | {d for v in edges.values() for d in v})
print(f'{len(mods)} top-level firmware modules\n')
print('=== outbound dependencies (module -> module, distinct symbols) ===')
for m in mods:
    deps = edges.get(m, {})
    if not deps:
        print(f'  {m:<16} (none)')
        continue
    parts = ', '.join(f'{d}({len(s)})' for d, s in sorted(deps.items(), key=lambda kv: -len(kv[1])))
    print(f'  {m:<16} -> {parts}')

print('\n=== fan-in (who depends on each module) ===')
for m in mods:
    ins = sorted(s for s, v in edges.items() if m in v)
    print(f'  {m:<16} <- {", ".join(ins) if ins else "(nobody)"}')
