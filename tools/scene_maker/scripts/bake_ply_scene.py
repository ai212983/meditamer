#!/usr/bin/env python3
"""Bake a simple 3D mesh scene into map passes for scene_maker.

Currently supports ASCII PLY meshes with vertex (x,y,z) and triangular faces.
Designed for deterministic offline map baking on development machines.
"""

from __future__ import annotations

import argparse
import math
from pathlib import Path

import numpy as np
from PIL import Image


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Bake map passes from an ASCII PLY mesh")
    p.add_argument("--mesh", type=Path, required=True, help="Input ASCII .ply mesh path")
    p.add_argument("--out-dir", type=Path, required=True, help="Output map directory")
    p.add_argument("--width", type=int, default=600)
    p.add_argument("--height", type=int, default=600)
    p.add_argument("--yaw-deg", type=float, default=26.0)
    p.add_argument("--pitch-deg", type=float, default=-8.0)
    p.add_argument("--distance", type=float, default=2.9)
    p.add_argument("--fov-deg", type=float, default=44.0)
    p.add_argument("--seed", type=int, default=1337)
    return p.parse_args()


def load_ascii_ply(path: Path) -> tuple[np.ndarray, np.ndarray]:
    lines = path.read_text(encoding="utf-8", errors="ignore").splitlines()
    if not lines or lines[0].strip() != "ply":
        raise ValueError(f"{path} is not a PLY file")

    if len(lines) < 3 or "format ascii" not in lines[1]:
        raise ValueError(
            "Only ASCII PLY is supported by this baker. "
            "Use happy_vrip_res4.ply from the Stanford bundle."
        )

    vertex_count = 0
    face_count = 0
    header_end = -1
    for i, line in enumerate(lines):
        s = line.strip()
        if s.startswith("element vertex"):
            vertex_count = int(s.split()[-1])
        elif s.startswith("element face"):
            face_count = int(s.split()[-1])
        elif s == "end_header":
            header_end = i
            break

    if header_end < 0:
        raise ValueError("PLY end_header not found")
    if vertex_count <= 0 or face_count <= 0:
        raise ValueError("PLY missing vertices/faces")

    start = header_end + 1
    vertex_lines = lines[start : start + vertex_count]
    face_lines = lines[start + vertex_count : start + vertex_count + face_count]

    vertices = np.empty((vertex_count, 3), dtype=np.float32)
    for i, line in enumerate(vertex_lines):
        vals = line.split()
        vertices[i, 0] = float(vals[0])
        vertices[i, 1] = float(vals[1])
        vertices[i, 2] = float(vals[2])

    faces = []
    for line in face_lines:
        vals = line.split()
        n = int(vals[0])
        if n < 3:
            continue
        idx = [int(v) for v in vals[1 : 1 + n]]
        for j in range(1, n - 1):
            faces.append((idx[0], idx[j], idx[j + 1]))

    return vertices, np.asarray(faces, dtype=np.int32)


def normalize_mesh(vertices: np.ndarray) -> np.ndarray:
    v = vertices.copy()
    center = (v.min(axis=0) + v.max(axis=0)) * 0.5
    v -= center
    scale = np.max(np.linalg.norm(v, axis=1))
    if scale <= 1e-9:
        scale = 1.0
    v /= scale
    return v


def rotation_matrix(yaw_deg: float, pitch_deg: float) -> np.ndarray:
    yaw = math.radians(yaw_deg)
    pitch = math.radians(pitch_deg)
    cy, sy = math.cos(yaw), math.sin(yaw)
    cx, sx = math.cos(pitch), math.sin(pitch)

    ry = np.array([[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]], dtype=np.float32)
    rx = np.array([[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]], dtype=np.float32)
    return rx @ ry


def compute_vertex_normals(vertices: np.ndarray, faces: np.ndarray) -> np.ndarray:
    v0 = vertices[faces[:, 0]]
    v1 = vertices[faces[:, 1]]
    v2 = vertices[faces[:, 2]]
    face_normals = np.cross(v1 - v0, v2 - v0)
    face_len = np.linalg.norm(face_normals, axis=1, keepdims=True)
    face_normals = face_normals / np.clip(face_len, 1e-8, None)

    normals = np.zeros_like(vertices, dtype=np.float32)
    np.add.at(normals, faces[:, 0], face_normals)
    np.add.at(normals, faces[:, 1], face_normals)
    np.add.at(normals, faces[:, 2], face_normals)

    nlen = np.linalg.norm(normals, axis=1, keepdims=True)
    normals = normals / np.clip(nlen, 1e-8, None)
    return normals


def project_vertices(
    vertices_cam: np.ndarray, width: int, height: int, fov_deg: float
) -> tuple[np.ndarray, np.ndarray]:
    z = vertices_cam[:, 2]
    f = 0.5 * width / math.tan(math.radians(fov_deg) * 0.5)
    x = f * (vertices_cam[:, 0] / z) + (width * 0.5)
    y = (-f * (vertices_cam[:, 1] / z)) + (height * 0.5)
    screen = np.stack([x, y], axis=1)
    return screen.astype(np.float32), z.astype(np.float32)


def rasterize(
    vertices_cam: np.ndarray,
    normals_cam: np.ndarray,
    faces: np.ndarray,
    width: int,
    height: int,
    fov_deg: float,
) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    screen, zvals = project_vertices(vertices_cam, width, height, fov_deg)

    depth = np.full((height, width), np.inf, dtype=np.float32)
    normal_buf = np.zeros((height, width, 3), dtype=np.float32)
    valid = np.zeros((height, width), dtype=np.bool_)

    for i0, i1, i2 in faces:
        z0, z1, z2 = zvals[i0], zvals[i1], zvals[i2]
        if z0 <= 0.03 or z1 <= 0.03 or z2 <= 0.03:
            continue

        p0 = screen[i0]
        p1 = screen[i1]
        p2 = screen[i2]

        minx = max(int(math.floor(min(p0[0], p1[0], p2[0]))), 0)
        maxx = min(int(math.ceil(max(p0[0], p1[0], p2[0]))), width - 1)
        miny = max(int(math.floor(min(p0[1], p1[1], p2[1]))), 0)
        maxy = min(int(math.ceil(max(p0[1], p1[1], p2[1]))), height - 1)
        if minx > maxx or miny > maxy:
            continue

        area = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0])
        if abs(area) < 1e-8:
            continue

        xs = np.arange(minx, maxx + 1, dtype=np.float32)
        ys = np.arange(miny, maxy + 1, dtype=np.float32)
        xv, yv = np.meshgrid(xs, ys)

        w0 = (p1[0] - xv) * (p2[1] - yv) - (p1[1] - yv) * (p2[0] - xv)
        w1 = (p2[0] - xv) * (p0[1] - yv) - (p2[1] - yv) * (p0[0] - xv)
        w2 = (p0[0] - xv) * (p1[1] - yv) - (p0[1] - yv) * (p1[0] - xv)

        if area > 0:
            inside = (w0 >= 0.0) & (w1 >= 0.0) & (w2 >= 0.0)
        else:
            inside = (w0 <= 0.0) & (w1 <= 0.0) & (w2 <= 0.0)

        if not np.any(inside):
            continue

        inv_area = 1.0 / area
        b0 = w0 * inv_area
        b1 = w1 * inv_area
        b2 = w2 * inv_area
        z_interp = (b0 * z0) + (b1 * z1) + (b2 * z2)

        region_depth = depth[miny : maxy + 1, minx : maxx + 1]
        update = inside & (z_interp < region_depth)
        if not np.any(update):
            continue

        region_depth[update] = z_interp[update]
        valid[miny : maxy + 1, minx : maxx + 1][update] = True

        n0 = normals_cam[i0]
        n1 = normals_cam[i1]
        n2 = normals_cam[i2]
        nx = (b0 * n0[0]) + (b1 * n1[0]) + (b2 * n2[0])
        ny = (b0 * n0[1]) + (b1 * n1[1]) + (b2 * n2[1])
        nz = (b0 * n0[2]) + (b1 * n1[2]) + (b2 * n2[2])

        region_norm = normal_buf[miny : maxy + 1, minx : maxx + 1]
        region_norm[..., 0][update] = nx[update]
        region_norm[..., 1][update] = ny[update]
        region_norm[..., 2][update] = nz[update]

    nlen = np.linalg.norm(normal_buf, axis=2, keepdims=True)
    normal_buf = normal_buf / np.clip(nlen, 1e-8, None)
    normal_buf[~valid] = np.array([0.0, 0.0, 1.0], dtype=np.float32)

    return depth, normal_buf, valid


def to_u8(img: np.ndarray) -> np.ndarray:
    return np.clip(img, 0, 255).astype(np.uint8)


def hash_noise(width: int, height: int, seed: int) -> np.ndarray:
    yy, xx = np.indices((height, width), dtype=np.uint32)
    v = xx * np.uint32(1664525) + yy * np.uint32(1013904223) + np.uint32(seed)
    v ^= v >> np.uint32(13)
    v *= np.uint32(2246822519)
    v ^= v >> np.uint32(16)
    return (v & np.uint32(255)).astype(np.uint8)


def bake_maps(
    depth: np.ndarray,
    normal_buf: np.ndarray,
    valid: np.ndarray,
    width: int,
    height: int,
    seed: int,
) -> dict[str, np.ndarray]:
    if not np.any(valid):
        raise ValueError("No visible geometry in rasterization result")

    z_valid = depth[valid]
    z_min = float(np.min(z_valid))
    z_max = float(np.max(z_valid))
    z_span = max(z_max - z_min, 1e-6)

    depth_n = np.zeros_like(depth, dtype=np.float32)
    depth_n[valid] = (depth[valid] - z_min) / z_span
    depth_img = np.full((height, width), 255.0, dtype=np.float32)
    depth_img[valid] = depth_n[valid] * 255.0

    l_key = np.array([0.42, 0.35, 0.84], dtype=np.float32)
    l_fill = np.array([-0.55, 0.25, 0.79], dtype=np.float32)
    l_top = np.array([0.0, 1.0, 0.0], dtype=np.float32)
    l_key /= np.linalg.norm(l_key)
    l_fill /= np.linalg.norm(l_fill)
    view = np.array([0.0, 0.0, 1.0], dtype=np.float32)

    key = np.clip(np.sum(normal_buf * l_key.reshape(1, 1, 3), axis=2), 0.0, 1.0)
    fill = np.clip(np.sum(normal_buf * l_fill.reshape(1, 1, 3), axis=2), 0.0, 1.0)
    top = np.clip(np.sum(normal_buf * l_top.reshape(1, 1, 3), axis=2), 0.0, 1.0)
    ndotv = np.clip(np.sum(normal_buf * view.reshape(1, 1, 3), axis=2), 0.0, 1.0)
    rim = np.power(1.0 - ndotv, 2.0)

    depth_for_grad = depth_n.copy()
    gx = np.gradient(depth_for_grad, axis=1)
    gy = np.gradient(depth_for_grad, axis=0)
    grad_depth = np.sqrt(gx * gx + gy * gy)

    ngx = np.gradient(normal_buf[..., 0], axis=1)
    ngy = np.gradient(normal_buf[..., 1], axis=0)
    ngz = np.gradient(normal_buf[..., 2], axis=1)
    grad_norm = np.sqrt(ngx * ngx + ngy * ngy + ngz * ngz)
    cavity = np.clip(grad_norm * 6.0, 0.0, 1.0)

    nx_abs = np.abs(normal_buf[..., 0])
    nz = np.clip(normal_buf[..., 2], 0.0, 1.0)

    albedo = np.full((height, width), 255.0, dtype=np.float32)
    albedo_shape = 156.0 + 62.0 * nz + 24.0 * (1.0 - nx_abs) - 28.0 * cavity
    albedo[valid] = np.clip(albedo_shape[valid], 60.0, 240.0)

    light = np.full((height, width), 255.0, dtype=np.float32)
    light_mix = 28.0 + 150.0 * key + 58.0 * fill + 24.0 * top + 40.0 * rim
    light[valid] = np.clip(light_mix[valid], 12.0, 255.0)

    ao = np.full((height, width), 255.0, dtype=np.float32)
    ao_raw = 255.0 - np.clip(
        (grad_depth * 920.0) + (grad_norm * 220.0) + (cavity * 35.0),
        0.0,
        225.0,
    )
    ao[valid] = ao_raw[valid]

    edge = np.zeros((height, width), dtype=np.float32)
    edge_mix = (grad_depth * 1450.0) + (grad_norm * 260.0)
    edge[valid] = np.clip(edge_mix[valid], 0.0, 255.0)

    mask = np.full((height, width), 255.0, dtype=np.float32)

    noise = hash_noise(width, height, seed).astype(np.float32)
    stroke = np.full((height, width), 128.0, dtype=np.float32)
    stroke[valid] = np.clip(
        128.0
        + (noise[valid] - 128.0) * 0.52
        + (edge[valid] - 70.0) * 0.28
        - cavity[valid] * 24.0,
        0.0,
        255.0,
    )

    normal_x = np.full((height, width), 128.0, dtype=np.float32)
    normal_y = np.full((height, width), 128.0, dtype=np.float32)
    normal_x[valid] = np.clip((normal_buf[..., 0][valid] * 0.5 + 0.5) * 255.0, 0.0, 255.0)
    normal_y[valid] = np.clip((normal_buf[..., 1][valid] * 0.5 + 0.5) * 255.0, 0.0, 255.0)

    return {
        "albedo": to_u8(albedo),
        "light": to_u8(light),
        "ao": to_u8(ao),
        "depth": to_u8(depth_img),
        "edge": to_u8(edge),
        "mask": to_u8(mask),
        "stroke": to_u8(stroke),
        "normal_x": to_u8(normal_x),
        "normal_y": to_u8(normal_y),
    }


def save_maps(out_dir: Path, maps: dict[str, np.ndarray]) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    for name, img in maps.items():
        Image.fromarray(img, mode="L").save(out_dir / f"{name}.png")


def main() -> None:
    args = parse_args()
    vertices, faces = load_ascii_ply(args.mesh)

    vertices = normalize_mesh(vertices)
    rot = rotation_matrix(args.yaw_deg, args.pitch_deg)

    vertices_rot = vertices @ rot.T
    normals = compute_vertex_normals(vertices, faces)
    normals_rot = normals @ rot.T

    # Camera at origin looking +Z; push model in front of camera.
    vertices_cam = vertices_rot + np.array([0.0, -0.08, args.distance], dtype=np.float32)

    depth, normal_buf, valid = rasterize(
        vertices_cam, normals_rot, faces, args.width, args.height, args.fov_deg
    )
    maps = bake_maps(depth, normal_buf, valid, args.width, args.height, args.seed)
    save_maps(args.out_dir, maps)

    coverage = float(np.mean(valid)) * 100.0
    print(f"wrote maps to {args.out_dir}")
    print(f"mesh vertices={len(vertices)} faces={len(faces)} coverage={coverage:.2f}%")


if __name__ == "__main__":
    main()
