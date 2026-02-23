#!/usr/bin/env python3
"""Set up and render a Buddha scene in Blender via the Blender MCP socket.

Requires a running Blender instance with the Blender MCP addon listening on localhost:9876.
"""

from __future__ import annotations

import argparse
import json
import socket
import textwrap
from pathlib import Path


def recv_json(sock: socket.socket, timeout_s: float) -> dict:
    sock.settimeout(timeout_s)
    chunks: list[bytes] = []
    while True:
        chunk = sock.recv(8192)
        if not chunk:
            break
        chunks.append(chunk)
        try:
            return json.loads(b"".join(chunks).decode("utf-8"))
        except json.JSONDecodeError:
            continue

    if not chunks:
        raise RuntimeError("No data received from Blender MCP socket")
    raise RuntimeError("Received incomplete JSON payload from Blender MCP socket")


def send_command(host: str, port: int, command_type: str, params: dict, timeout_s: float) -> dict:
    payload = {"type": command_type, "params": params}
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.connect((host, port))
        sock.sendall(json.dumps(payload).encode("utf-8"))
        response = recv_json(sock, timeout_s)

    if response.get("status") != "success":
        raise RuntimeError(f"Blender MCP command failed: {response}")
    return response.get("result", {})


def parse_args() -> argparse.Namespace:
    root = Path(__file__).resolve().parents[3]
    default_mesh = root / "tools/scene_maker/assets/buddha_happy/happy_recon/happy_vrip_res2.ply"
    default_out = root / "tools/scene_viewer/out/buddha_blender"

    p = argparse.ArgumentParser(description="Set up a Blender Buddha scene through Blender MCP")
    p.add_argument("--host", default="localhost")
    p.add_argument("--port", type=int, default=9876)
    p.add_argument("--timeout", type=float, default=600.0)
    p.add_argument("--mesh", type=Path, default=default_mesh)
    p.add_argument("--out-dir", type=Path, default=default_out)
    p.add_argument("--width", type=int, default=600)
    p.add_argument("--height", type=int, default=600)
    p.add_argument("--samples-master", type=int, default=768)
    p.add_argument("--samples-variants", type=int, default=512)
    return p.parse_args()


def build_blender_code(args: argparse.Namespace, blend_path: Path, render_dir: Path) -> str:
    return textwrap.dedent(
        f"""
        import bpy
        import json
        import math
        import os
        from mathutils import Vector

        MESH_PATH = {json.dumps(str(args.mesh.resolve()))}
        RENDER_DIR = {json.dumps(str(render_dir.resolve()))}
        BLEND_PATH = {json.dumps(str(blend_path.resolve()))}
        WIDTH = {int(args.width)}
        HEIGHT = {int(args.height)}
        SAMPLES_MASTER = {int(args.samples_master)}
        SAMPLES_VARIANTS = {int(args.samples_variants)}

        os.makedirs(RENDER_DIR, exist_ok=True)
        os.makedirs(os.path.dirname(BLEND_PATH), exist_ok=True)


        def clear_scene():
            bpy.ops.object.select_all(action='SELECT')
            bpy.ops.object.delete(use_global=False)
            for block in bpy.data.meshes:
                if block.users == 0:
                    bpy.data.meshes.remove(block)
            for block in bpy.data.materials:
                if block.users == 0:
                    bpy.data.materials.remove(block)


        def look_at(obj, target):
            direction = Vector(target) - obj.location
            quat = direction.to_track_quat('-Z', 'Y')
            obj.rotation_euler = quat.to_euler()


        def set_sun_orientation(light_obj, sky_node, azimuth_deg, elevation_deg):
            az = math.radians(azimuth_deg)
            el = math.radians(elevation_deg)
            direction = Vector((
                math.cos(el) * math.sin(az),
                math.cos(el) * math.cos(az),
                math.sin(el),
            ))
            light_obj.rotation_euler = direction.to_track_quat('-Z', 'Y').to_euler()
            sky_node.sun_rotation = az
            sky_node.sun_elevation = el


        clear_scene()

        scene = bpy.context.scene
        scene.render.engine = 'CYCLES'
        scene.cycles.device = 'CPU'
        scene.cycles.use_adaptive_sampling = True
        scene.cycles.samples = SAMPLES_MASTER
        scene.cycles.max_bounces = 8
        scene.cycles.diffuse_bounces = 4
        scene.cycles.glossy_bounces = 2
        scene.cycles.transparent_max_bounces = 4
        scene.cycles.caustics_reflective = False
        scene.cycles.caustics_refractive = False
        scene.render.resolution_x = WIDTH
        scene.render.resolution_y = HEIGHT
        scene.render.resolution_percentage = 100
        scene.render.image_settings.file_format = 'PNG'
        scene.render.image_settings.color_mode = 'RGB'
        scene.render.image_settings.color_depth = '8'
        scene.view_settings.view_transform = 'Standard'
        scene.view_settings.look = 'None'
        scene.view_settings.exposure = 0.0
        scene.view_settings.gamma = 1.0

        world = bpy.data.worlds.new('BuddhaWorld')
        scene.world = world
        world.use_nodes = True
        wn = world.node_tree.nodes
        wl = world.node_tree.links
        wn.clear()

        world_out = wn.new(type='ShaderNodeOutputWorld')
        background = wn.new(type='ShaderNodeBackground')
        sky = wn.new(type='ShaderNodeTexSky')
        if hasattr(sky, 'bl_rna') and 'sky_type' in sky.bl_rna.properties:
            enum_items = [it.identifier for it in sky.bl_rna.properties['sky_type'].enum_items]
            for candidate in ('MULTIPLE_SCATTERING', 'NISHITA', 'PREETHAM', 'HOSEK_WILKIE', 'SINGLE_SCATTERING'):
                if candidate in enum_items:
                    sky.sky_type = candidate
                    break
        if hasattr(sky, 'altitude'):
            sky.altitude = 1.0
        if hasattr(sky, 'air_density'):
            sky.air_density = 1.2
        if hasattr(sky, 'dust_density'):
            sky.dust_density = 1.6
        if hasattr(sky, 'ozone_density'):
            sky.ozone_density = 1.0
        background.inputs['Strength'].default_value = 0.72

        volume = wn.new(type='ShaderNodeVolumePrincipled')
        volume.inputs['Density'].default_value = 0.002
        volume.inputs['Anisotropy'].default_value = 0.15

        wl.new(sky.outputs['Color'], background.inputs['Color'])
        wl.new(background.outputs['Background'], world_out.inputs['Surface'])
        wl.new(volume.outputs['Volume'], world_out.inputs['Volume'])

        imported_obj = None
        existing = set(bpy.data.objects)
        try:
            bpy.ops.import_mesh.ply(filepath=MESH_PATH)
        except Exception:
            bpy.ops.wm.ply_import(filepath=MESH_PATH)

        new_objs = [o for o in bpy.data.objects if o not in existing and o.type == 'MESH']
        if new_objs:
            imported_obj = max(new_objs, key=lambda o: len(o.data.vertices))
        else:
            mesh_objs = [o for o in bpy.data.objects if o.type == 'MESH']
            if mesh_objs:
                imported_obj = max(mesh_objs, key=lambda o: len(o.data.vertices))

        if imported_obj is None:
            raise RuntimeError(f'Unable to import mesh from {{MESH_PATH}}')

        imported_obj.name = 'Buddha'
        bpy.ops.object.select_all(action='DESELECT')
        imported_obj.select_set(True)
        bpy.context.view_layer.objects.active = imported_obj
        bpy.ops.object.shade_smooth()

        if all(m.type != 'WEIGHTED_NORMAL' for m in imported_obj.modifiers):
            mod = imported_obj.modifiers.new(name='WeightedNormal', type='WEIGHTED_NORMAL')
            mod.keep_sharp = True

        bpy.context.view_layer.update()
        corners = [imported_obj.matrix_world @ Vector(corner) for corner in imported_obj.bound_box]
        min_v = Vector((min(v.x for v in corners), min(v.y for v in corners), min(v.z for v in corners)))
        max_v = Vector((max(v.x for v in corners), max(v.y for v in corners), max(v.z for v in corners)))
        height = max_v.z - min_v.z
        if height > 1e-6:
            target_height = 1.72
            scale = target_height / height
            imported_obj.scale = imported_obj.scale * scale

        bpy.context.view_layer.update()
        corners = [imported_obj.matrix_world @ Vector(corner) for corner in imported_obj.bound_box]
        min_v = Vector((min(v.x for v in corners), min(v.y for v in corners), min(v.z for v in corners)))
        max_v = Vector((max(v.x for v in corners), max(v.y for v in corners), max(v.z for v in corners)))
        center_xy = Vector(((min_v.x + max_v.x) * 0.5, (min_v.y + max_v.y) * 0.5, 0.0))
        imported_obj.location.x -= center_xy.x
        imported_obj.location.y -= center_xy.y
        imported_obj.location.z -= min_v.z
        imported_obj.rotation_euler[2] = math.radians(14.0)

        buddha_mat = bpy.data.materials.new(name='BuddhaMaterial')
        buddha_mat.use_nodes = True
        bsdf = buddha_mat.node_tree.nodes.get('Principled BSDF')
        bsdf.inputs['Base Color'].default_value = (0.66, 0.64, 0.60, 1.0)
        bsdf.inputs['Roughness'].default_value = 0.78
        if imported_obj.data.materials:
            imported_obj.data.materials[0] = buddha_mat
        else:
            imported_obj.data.materials.append(buddha_mat)

        bpy.ops.mesh.primitive_plane_add(size=14.0, location=(0.0, 0.0, 0.0))
        ground = bpy.context.active_object
        ground.name = 'Ground'

        ground_mat = bpy.data.materials.new(name='GroundMaterial')
        ground_mat.use_nodes = True
        gbsdf = ground_mat.node_tree.nodes.get('Principled BSDF')
        gbsdf.inputs['Base Color'].default_value = (0.93, 0.92, 0.89, 1.0)
        gbsdf.inputs['Roughness'].default_value = 0.98
        if ground.data.materials:
            ground.data.materials[0] = ground_mat
        else:
            ground.data.materials.append(ground_mat)

        sun_data = bpy.data.lights.new(name='SunMainData', type='SUN')
        sun_data.energy = 4.4
        sun_data.angle = math.radians(1.1)
        sun_obj = bpy.data.objects.new(name='SunMain', object_data=sun_data)
        bpy.context.collection.objects.link(sun_obj)

        fill_data = bpy.data.lights.new(name='FillAreaData', type='AREA')
        fill_data.energy = 220.0
        fill_data.shape = 'RECTANGLE'
        fill_data.size = 3.4
        fill_data.size_y = 2.2
        fill_data.color = (1.0, 0.98, 0.93)
        fill_obj = bpy.data.objects.new(name='FillArea', object_data=fill_data)
        fill_obj.location = (0.0, -3.3, 2.1)
        bpy.context.collection.objects.link(fill_obj)
        look_at(fill_obj, (0.0, 0.0, 0.9))

        rim_data = bpy.data.lights.new(name='RimData', type='SPOT')
        rim_data.energy = 120.0
        rim_data.spot_size = math.radians(56.0)
        rim_data.spot_blend = 0.35
        rim_data.color = (0.92, 0.95, 1.0)
        rim_obj = bpy.data.objects.new(name='RimLight', object_data=rim_data)
        rim_obj.location = (1.9, 2.0, 2.3)
        bpy.context.collection.objects.link(rim_obj)
        look_at(rim_obj, (0.0, 0.0, 0.8))

        cam_data = bpy.data.cameras.new(name='MainCameraData')
        cam_data.lens = 52
        cam = bpy.data.objects.new(name='MainCamera', object_data=cam_data)
        cam.location = (0.0, -3.0, 1.28)
        bpy.context.collection.objects.link(cam)
        look_at(cam, (0.0, 0.0, 0.92))
        scene.camera = cam

        def render_variant(file_name, azimuth_deg, elevation_deg, fog_density, samples):
            set_sun_orientation(sun_obj, sky, azimuth_deg, elevation_deg)
            volume.inputs['Density'].default_value = fog_density
            scene.cycles.samples = samples
            scene.render.filepath = os.path.join(RENDER_DIR, file_name)
            bpy.ops.render.render(write_still=True)

        render_variant('master_scene_geometry_minimal.png', 160.0, 28.0, 0.0003, SAMPLES_MASTER)
        render_variant('daylight_reference.png', 118.0, 36.0, 0.0035, SAMPLES_VARIANTS)
        render_variant('evening_reference.png', 248.0, 12.0, 0.0105, SAMPLES_VARIANTS)

        scene.frame_start = 1
        scene.frame_end = 120
        scene.frame_set(1)
        set_sun_orientation(sun_obj, sky, 110.0, 32.0)
        volume.inputs['Density'].default_value = 0.0035
        sun_obj.keyframe_insert(data_path='rotation_euler', frame=1)
        sky.keyframe_insert(data_path='sun_rotation', frame=1)
        sky.keyframe_insert(data_path='sun_elevation', frame=1)
        volume.inputs['Density'].keyframe_insert(data_path='default_value', frame=1)

        scene.frame_set(120)
        set_sun_orientation(sun_obj, sky, 255.0, 10.0)
        volume.inputs['Density'].default_value = 0.0105
        sun_obj.keyframe_insert(data_path='rotation_euler', frame=120)
        sky.keyframe_insert(data_path='sun_rotation', frame=120)
        sky.keyframe_insert(data_path='sun_elevation', frame=120)
        volume.inputs['Density'].keyframe_insert(data_path='default_value', frame=120)

        bpy.ops.wm.save_as_mainfile(filepath=BLEND_PATH)

        summary = {{
            'blend': BLEND_PATH,
            'render_dir': RENDER_DIR,
            'renders': [
                'master_scene_geometry_minimal.png',
                'daylight_reference.png',
                'evening_reference.png',
            ],
            'camera': list(cam.location),
            'mesh': imported_obj.name,
        }}
        print(json.dumps(summary))
        """
    )


def main() -> None:
    args = parse_args()

    if not args.mesh.exists():
        raise SystemExit(f"Mesh file not found: {args.mesh}")

    out_dir = args.out_dir.resolve()
    render_dir = out_dir / "renders"
    debug_dir = out_dir / "debug"
    blend_path = out_dir / "blender" / "buddha_scene.blend"
    render_dir.mkdir(parents=True, exist_ok=True)
    debug_dir.mkdir(parents=True, exist_ok=True)

    code = build_blender_code(args, blend_path=blend_path, render_dir=render_dir)
    result = send_command(
        args.host,
        args.port,
        "execute_code",
        {"code": code},
        timeout_s=args.timeout,
    )

    scene_info = send_command(args.host, args.port, "get_scene_info", {}, timeout_s=30.0)
    (debug_dir / "scene_info.json").write_text(json.dumps(scene_info, indent=2), encoding="utf-8")
    (debug_dir / "execute_result.json").write_text(json.dumps(result, indent=2), encoding="utf-8")

    manifest = {
        "mesh": str(args.mesh.resolve()),
        "blend": str(blend_path),
        "render_dir": str(render_dir),
        "renders": [
            "master_scene_geometry_minimal.png",
            "daylight_reference.png",
            "evening_reference.png",
        ],
    }
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")

    print(f"Blender scene configured and rendered to: {render_dir}")
    print(f"Blend file saved at: {blend_path}")


if __name__ == "__main__":
    main()
