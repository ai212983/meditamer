#!/usr/bin/env python3
"""Incremental Buddha scene setup via Blender MCP socket.

Usage examples:
  python3 tools/scene_maker/scripts/buddha_blender_stepper.py --step 1
  python3 tools/scene_maker/scripts/buddha_blender_stepper.py --step 2
  python3 tools/scene_maker/scripts/buddha_blender_stepper.py --step 2b
  python3 tools/scene_maker/scripts/buddha_blender_stepper.py --step 2c
  python3 tools/scene_maker/scripts/buddha_blender_stepper.py --step 2d
  python3 tools/scene_maker/scripts/buddha_blender_stepper.py --step 3
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
    p = argparse.ArgumentParser(description="Run incremental Blender scene setup steps")
    p.add_argument("--step", type=str, choices=["1", "2", "2b", "2c", "2d", "3"], required=True)
    p.add_argument("--host", default="localhost")
    p.add_argument("--port", type=int, default=9876)
    p.add_argument("--timeout", type=float, default=300.0)
    p.add_argument(
        "--mesh",
        type=Path,
        default=root / "tools/scene_maker/assets/buddha_happy/happy_recon/happy_vrip_res2.ply",
    )
    p.add_argument(
        "--out-dir",
        type=Path,
        default=root / "tools/scene_viewer/out/buddha_blender_steps",
    )
    p.add_argument("--width", type=int, default=600)
    p.add_argument("--height", type=int, default=600)
    p.add_argument("--samples", type=int, default=128)
    p.add_argument("--engine", type=str, choices=["eevee", "cycles"], default="eevee")
    return p.parse_args()


def code_header(
    mesh: Path, out_dir: Path, blend_path: Path, width: int, height: int, samples: int, engine: str
) -> str:
    return textwrap.dedent(
        f"""
        import bpy
        import json
        import math
        import os
        from mathutils import Vector

        MESH_PATH = {json.dumps(str(mesh.resolve()))}
        OUT_DIR = {json.dumps(str(out_dir.resolve()))}
        RENDER_DIR = {json.dumps(str((out_dir / 'renders').resolve()))}
        BLEND_PATH = {json.dumps(str(blend_path.resolve()))}
        WIDTH = {width}
        HEIGHT = {height}
        SAMPLES = {samples}
        ENGINE = {json.dumps(engine)}

        os.makedirs(RENDER_DIR, exist_ok=True)
        os.makedirs(os.path.dirname(BLEND_PATH), exist_ok=True)
        if bpy.app.version < (5, 0, 0):
            raise RuntimeError(f'This stepper requires Blender 5.0+, got {{bpy.app.version_string}}')

        def set_render_defaults(samples):
            scene = bpy.context.scene
            if ENGINE == 'eevee':
                enum_items = [it.identifier for it in scene.render.bl_rna.properties['engine'].enum_items]
                if 'BLENDER_EEVEE_NEXT' in enum_items:
                    scene.render.engine = 'BLENDER_EEVEE_NEXT'
                elif 'BLENDER_EEVEE' in enum_items:
                    scene.render.engine = 'BLENDER_EEVEE'
                else:
                    scene.render.engine = 'CYCLES'

                if scene.render.engine != 'CYCLES':
                    if hasattr(scene, 'eevee'):
                        eev = scene.eevee
                        if hasattr(eev, 'taa_render_samples'):
                            eev.taa_render_samples = max(8, samples)
                        if hasattr(eev, 'taa_samples'):
                            eev.taa_samples = max(8, samples)
                        if hasattr(eev, 'use_gtao'):
                            eev.use_gtao = True
                        if hasattr(eev, 'gtao_distance'):
                            eev.gtao_distance = 0.18
                        if hasattr(eev, 'use_bloom'):
                            eev.use_bloom = False
                else:
                    scene.cycles.device = 'CPU'
                    scene.cycles.use_adaptive_sampling = True
                    scene.cycles.use_denoising = True
                    scene.cycles.samples = samples
                    scene.cycles.max_bounces = 6
                    scene.cycles.diffuse_bounces = 3
                    scene.cycles.glossy_bounces = 2
            else:
                scene.render.engine = 'CYCLES'
                scene.cycles.device = 'CPU'
                scene.cycles.use_adaptive_sampling = True
                scene.cycles.use_denoising = True
                scene.cycles.samples = samples
                scene.cycles.max_bounces = 6
                scene.cycles.diffuse_bounces = 3
                scene.cycles.glossy_bounces = 2
            scene.render.resolution_x = WIDTH
            scene.render.resolution_y = HEIGHT
            scene.render.resolution_percentage = 100
            scene.render.image_settings.file_format = 'PNG'
            scene.render.image_settings.color_mode = 'RGB'
            scene.render.image_settings.color_depth = '8'

        def look_at(obj, target):
            direction = Vector(target) - obj.location
            obj.rotation_euler = direction.to_track_quat('-Z', 'Y').to_euler()

        def pick_sky_model(sky):
            try:
                enum_items = [it.identifier for it in sky.bl_rna.properties['sky_type'].enum_items]
            except Exception:
                return
            for candidate in ('MULTIPLE_SCATTERING', 'NISHITA', 'PREETHAM', 'HOSEK_WILKIE', 'SINGLE_SCATTERING'):
                if candidate in enum_items:
                    sky.sky_type = candidate
                    return

        def set_sun_orientation(sun_obj, sky_node, azimuth_deg, elevation_deg):
            az = math.radians(azimuth_deg)
            el = math.radians(elevation_deg)
            direction = Vector((
                math.cos(el) * math.sin(az),
                math.cos(el) * math.cos(az),
                math.sin(el),
            ))
            sun_obj.rotation_euler = direction.to_track_quat('-Z', 'Y').to_euler()
            if hasattr(sky_node, 'sun_rotation'):
                sky_node.sun_rotation = az
            if hasattr(sky_node, 'sun_elevation'):
                sky_node.sun_elevation = el

        def get_object(name):
            return bpy.data.objects.get(name)

        def require_object(name):
            obj = bpy.data.objects.get(name)
            if obj is None:
                raise RuntimeError(f'Missing required object: {{name}}')
            return obj

        def capture_transform(name):
            obj = bpy.data.objects.get(name)
            if obj is None:
                return None
            return dict(
                location=tuple(obj.location),
                rotation=tuple(obj.rotation_euler),
                scale=tuple(obj.scale),
            )

        def restore_transform(name, snapshot):
            if snapshot is None:
                return
            obj = bpy.data.objects.get(name)
            if obj is None:
                return
            obj.location = snapshot['location']
            obj.rotation_euler = snapshot['rotation']
            obj.scale = snapshot['scale']

        def remove_object_if_exists(name):
            obj = bpy.data.objects.get(name)
            if obj is not None:
                bpy.data.objects.remove(obj, do_unlink=True)

        def assign_single_material(obj, mat):
            if obj.data.materials:
                obj.data.materials[0] = mat
                while len(obj.data.materials) > 1:
                    obj.data.materials.pop(index=len(obj.data.materials) - 1)
            else:
                obj.data.materials.append(mat)

        def ensure_ribbon_material():
            mat = bpy.data.materials.get('InkRibbonMaterial')
            if mat is None:
                mat = bpy.data.materials.new(name='InkRibbonMaterial')
            mat.use_nodes = True
            mat.blend_method = 'BLEND'
            if hasattr(mat, 'shadow_method'):
                mat.shadow_method = 'NONE'
            nt = mat.node_tree
            nodes = nt.nodes
            links = nt.links
            nodes.clear()

            out = nodes.new(type='ShaderNodeOutputMaterial')
            mix_shader = nodes.new(type='ShaderNodeMixShader')
            transparent = nodes.new(type='ShaderNodeBsdfTransparent')
            princ = nodes.new(type='ShaderNodeBsdfPrincipled')
            princ.inputs['Base Color'].default_value = (0.06, 0.06, 0.06, 1.0)
            princ.inputs['Roughness'].default_value = 0.98
            princ.inputs['Specular IOR Level'].default_value = 0.0
            princ.inputs['Alpha'].default_value = 1.0

            texcoord = nodes.new(type='ShaderNodeTexCoord')
            mapping = nodes.new(type='ShaderNodeMapping')
            mapping.inputs['Scale'].default_value = (1.0, 7.0, 1.0)
            wave = nodes.new(type='ShaderNodeTexWave')
            wave.wave_type = 'BANDS'
            wave.bands_direction = 'Y'
            wave.inputs['Scale'].default_value = 1.6
            wave.inputs['Distortion'].default_value = 0.55
            noise = nodes.new(type='ShaderNodeTexNoise')
            noise.inputs['Scale'].default_value = 1.8
            noise.inputs['Detail'].default_value = 1.4
            noise.inputs['Roughness'].default_value = 0.30
            mult = nodes.new(type='ShaderNodeMixRGB')
            mult.blend_type = 'MULTIPLY'
            mult.inputs['Fac'].default_value = 0.18
            base_ink = nodes.new(type='ShaderNodeRGB')
            base_ink.outputs['Color'].default_value = (0.075, 0.075, 0.075, 1.0)
            stroke_variation = nodes.new(type='ShaderNodeValToRGB')
            stroke_variation.color_ramp.elements[0].position = 0.40
            stroke_variation.color_ramp.elements[0].color = (0.92, 0.92, 0.92, 1.0)
            stroke_variation.color_ramp.elements[1].position = 0.76
            stroke_variation.color_ramp.elements[1].color = (1.04, 1.04, 1.04, 1.0)
            alpha_mul = nodes.new(type='ShaderNodeMath')
            alpha_mul.operation = 'MULTIPLY'
            alpha_mul.inputs[1].default_value = 0.72
            alpha_ramp = nodes.new(type='ShaderNodeValToRGB')
            alpha_ramp.color_ramp.interpolation = 'B_SPLINE'
            alpha_ramp.color_ramp.elements[0].position = 0.26
            alpha_ramp.color_ramp.elements[0].color = (0.0, 0.0, 0.0, 1.0)
            alpha_ramp.color_ramp.elements[1].position = 0.74
            alpha_ramp.color_ramp.elements[1].color = (1.0, 1.0, 1.0, 1.0)

            links.new(transparent.outputs['BSDF'], mix_shader.inputs[1])
            links.new(princ.outputs['BSDF'], mix_shader.inputs[2])
            links.new(mix_shader.outputs['Shader'], out.inputs['Surface'])
            links.new(texcoord.outputs['Object'], mapping.inputs['Vector'])
            links.new(mapping.outputs['Vector'], wave.inputs['Vector'])
            links.new(mapping.outputs['Vector'], noise.inputs['Vector'])
            links.new(wave.outputs['Color'], stroke_variation.inputs['Fac'])
            links.new(base_ink.outputs['Color'], mult.inputs['Color1'])
            links.new(stroke_variation.outputs['Color'], mult.inputs['Color2'])
            links.new(mult.outputs['Color'], princ.inputs['Base Color'])
            links.new(wave.outputs['Color'], alpha_mul.inputs[0])
            links.new(alpha_mul.outputs['Value'], alpha_ramp.inputs['Fac'])
            links.new(alpha_ramp.outputs['Color'], mix_shader.inputs['Fac'])

            return mat

        def ensure_ribbon_object(source_obj):
            remove_object_if_exists('BuddhaRibbons')
            rib = source_obj.copy()
            rib.data = source_obj.data.copy()
            rib.name = 'BuddhaRibbons'
            bpy.context.collection.objects.link(rib)
            rib.matrix_world = source_obj.matrix_world.copy()
            rib.parent = None
            rib.scale = (
                source_obj.scale[0] * 1.003,
                source_obj.scale[1] * 1.003,
                source_obj.scale[2] * 1.003,
            )
            if hasattr(rib, 'visible_shadow'):
                rib.visible_shadow = False
            if hasattr(rib, 'visible_glossy'):
                rib.visible_glossy = False

            rib_mat = ensure_ribbon_material()
            assign_single_material(rib, rib_mat)

            dec = rib.modifiers.new(name='InkRibbonDecimate', type='DECIMATE')
            dec.ratio = 0.16

            geo_mod = rib.modifiers.new(name='InkRibbonGeo', type='NODES')
            node_group = bpy.data.node_groups.new('InkRibbonGeoGroup', 'GeometryNodeTree')
            geo_mod.node_group = node_group
            nodes = node_group.nodes
            links = node_group.links
            nodes.clear()

            group_in = nodes.new(type='NodeGroupInput')
            group_out = nodes.new(type='NodeGroupOutput')
            node_group.interface.new_socket(
                name='Geometry', in_out='INPUT', socket_type='NodeSocketGeometry'
            )
            node_group.interface.new_socket(
                name='Geometry', in_out='OUTPUT', socket_type='NodeSocketGeometry'
            )

            mesh_to_curve = nodes.new(type='GeometryNodeMeshToCurve')
            resample = nodes.new(type='GeometryNodeResampleCurve')
            if hasattr(resample, 'mode'):
                resample.mode = 'LENGTH'
            if 'Length' in resample.inputs:
                resample.inputs['Length'].default_value = 0.040
            elif 'Count' in resample.inputs:
                resample.inputs['Count'].default_value = 36
            spline_type = nodes.new(type='GeometryNodeCurveSplineType')
            spline_type.spline_type = 'BEZIER'
            subdivide = nodes.new(type='GeometryNodeSubdivideCurve')
            subdivide.inputs['Cuts'].default_value = 2
            spline_param = nodes.new(type='GeometryNodeSplineParameter')
            noise = nodes.new(type='ShaderNodeTexNoise')
            noise.inputs['Scale'].default_value = 2.0
            noise.inputs['Detail'].default_value = 1.6
            fac_2 = nodes.new(type='ShaderNodeMath')
            fac_2.operation = 'MULTIPLY'
            fac_2.inputs[1].default_value = 2.0
            fac_center = nodes.new(type='ShaderNodeMath')
            fac_center.operation = 'SUBTRACT'
            fac_center.inputs[1].default_value = 1.0
            fac_abs = nodes.new(type='ShaderNodeMath')
            fac_abs.operation = 'ABSOLUTE'
            fac_taper = nodes.new(type='ShaderNodeMath')
            fac_taper.operation = 'SUBTRACT'
            fac_taper.inputs[0].default_value = 1.0
            fac_pow = nodes.new(type='ShaderNodeMath')
            fac_pow.operation = 'POWER'
            fac_pow.inputs[1].default_value = 0.72
            noise_gain = nodes.new(type='ShaderNodeMath')
            noise_gain.operation = 'MULTIPLY_ADD'
            noise_gain.inputs[1].default_value = 0.65
            noise_gain.inputs[2].default_value = 0.35
            profile = nodes.new(type='ShaderNodeMath')
            profile.operation = 'MULTIPLY'
            radius_math = nodes.new(type='ShaderNodeMath')
            radius_math.operation = 'MULTIPLY_ADD'
            radius_math.inputs[1].default_value = 0.0025
            radius_math.inputs[2].default_value = 0.00035
            set_radius = nodes.new(type='GeometryNodeSetCurveRadius')
            curve_circle = nodes.new(type='GeometryNodeCurvePrimitiveCircle')
            curve_circle.mode = 'RADIUS'
            curve_circle.inputs['Radius'].default_value = 0.0012
            curve_to_mesh = nodes.new(type='GeometryNodeCurveToMesh')
            set_mat = nodes.new(type='GeometryNodeSetMaterial')
            set_mat.inputs['Material'].default_value = rib_mat

            edge_angle = nodes.new(type='GeometryNodeInputMeshEdgeAngle')
            compare = nodes.new(type='FunctionNodeCompare')
            compare.data_type = 'FLOAT'
            compare.operation = 'GREATER_THAN'
            compare.inputs[1].default_value = 0.78

            links.new(edge_angle.outputs[0], compare.inputs[0])
            links.new(compare.outputs[0], mesh_to_curve.inputs['Selection'])

            links.new(group_in.outputs['Geometry'], mesh_to_curve.inputs['Mesh'])
            links.new(mesh_to_curve.outputs['Curve'], resample.inputs['Curve'])
            links.new(resample.outputs['Curve'], spline_type.inputs['Curve'])
            links.new(spline_type.outputs['Curve'], subdivide.inputs['Curve'])
            links.new(subdivide.outputs['Curve'], set_radius.inputs['Curve'])
            links.new(spline_param.outputs['Factor'], noise.inputs['Vector'])
            links.new(spline_param.outputs['Factor'], fac_2.inputs[0])
            links.new(fac_2.outputs['Value'], fac_center.inputs[0])
            links.new(fac_center.outputs['Value'], fac_abs.inputs[0])
            links.new(fac_abs.outputs['Value'], fac_taper.inputs[1])
            links.new(fac_taper.outputs['Value'], fac_pow.inputs[0])
            links.new(noise.outputs['Fac'], noise_gain.inputs[0])
            links.new(fac_pow.outputs['Value'], profile.inputs[0])
            links.new(noise_gain.outputs['Value'], profile.inputs[1])
            links.new(profile.outputs['Value'], radius_math.inputs[0])
            links.new(radius_math.outputs['Value'], set_radius.inputs['Radius'])
            links.new(set_radius.outputs['Curve'], curve_to_mesh.inputs['Curve'])
            links.new(curve_circle.outputs['Curve'], curve_to_mesh.inputs['Profile Curve'])
            links.new(curve_to_mesh.outputs['Mesh'], set_mat.inputs['Geometry'])
            links.new(set_mat.outputs['Geometry'], group_out.inputs['Geometry'])
            return rib

        def remove_guide_strokes():
            remove_object_if_exists('BuddhaGuideStrokes')
            curve_data = bpy.data.curves.get('BuddhaGuideStrokeCurve')
            if curve_data is not None and curve_data.users == 0:
                bpy.data.curves.remove(curve_data)
            mat = bpy.data.materials.get('InkGuideStrokeMaterial')
            if mat is not None and mat.users == 0:
                bpy.data.materials.remove(mat)

        def clear_compositor():
            scene = bpy.context.scene
            nt = getattr(scene, 'node_tree', None)
            if nt is not None:
                nt.nodes.clear()
                nt.links.clear()
            if hasattr(scene, 'compositing_node_group'):
                comp_group = scene.compositing_node_group
                if comp_group is not None:
                    comp_group.nodes.clear()
                    comp_group.links.clear()
                    scene.compositing_node_group = None
                    if comp_group.users == 0:
                        bpy.data.node_groups.remove(comp_group)
            scene.use_nodes = False

        def get_compositor_tree(create=False):
            scene = bpy.context.scene
            nt = getattr(scene, 'node_tree', None)
            if nt is not None:
                return nt
            if hasattr(scene, 'compositing_node_group'):
                if scene.compositing_node_group is None and create:
                    scene.compositing_node_group = bpy.data.node_groups.new(
                        'SumiCompositorTree', 'CompositorNodeTree'
                    )
                return scene.compositing_node_group
            return None

        def ensure_paper_noise_image():
            img = bpy.data.images.get('SumiPaperNoise')
            size = 256
            if img is None:
                img = bpy.data.images.new('SumiPaperNoise', width=size, height=size, alpha=False, float_buffer=False)
            if img.size[0] != size or img.size[1] != size:
                img.scale(size, size)
            if not img.get('sumi_noise_ready'):
                pixels = [0.0] * (size * size * 4)
                i = 0
                for y in range(size):
                    for x in range(size):
                        h0 = ((x * 73856093) ^ (y * 19349663) ^ 0x9E3779B9) & 0xFFFFFFFF
                        h1 = (((x // 16) * 83492791) ^ ((y // 16) * 2654435761)) & 0xFFFFFFFF
                        n0 = (h0 & 1023) / 1023.0
                        n1 = (h1 & 255) / 255.0
                        tone = 0.5 + (n0 - 0.5) * 0.18 + (n1 - 0.5) * 0.14
                        tone = max(0.0, min(1.0, tone))
                        pixels[i] = tone
                        pixels[i + 1] = tone
                        pixels[i + 2] = tone
                        pixels[i + 3] = 1.0
                        i += 4
                img.pixels.foreach_set(pixels)
                img['sumi_noise_ready'] = True
                img.update()
            return img

        def estimate_depth_range(obj, cam):
            bpy.context.view_layer.update()
            corners = [obj.matrix_world @ Vector(corner) for corner in obj.bound_box]
            cam_inv = cam.matrix_world.inverted()
            depths = []
            for p in corners:
                p_cam = cam_inv @ p
                depths.append(max(0.0, -p_cam.z))
            if not depths:
                return (1.0, 5.0)
            near = max(0.01, min(depths) * 0.86)
            far = max(near + 0.5, max(depths) * 1.14)
            return (near, far)

        def setup_sumi_e_compositor(source_obj):
            scene = bpy.context.scene
            cam = scene.camera
            if cam is None:
                raise RuntimeError('Camera is required for sumi-e compositor')

            view_layer = bpy.context.view_layer
            if hasattr(view_layer, 'use_pass_z'):
                view_layer.use_pass_z = True

            scene.use_nodes = True
            nt = get_compositor_tree(create=True)
            if nt is None:
                raise RuntimeError('Compositor node tree unavailable after enabling scene.use_nodes')
            nodes = nt.nodes
            links = nt.links
            nodes.clear()
            links.clear()

            rl = nodes.new(type='CompositorNodeRLayers')
            base_bw = nodes.new(type='CompositorNodeRGBToBW')
            bw_to_img = nodes.new(type='CompositorNodeCombineColor')
            wash_boost = nodes.new(type='CompositorNodeBrightContrast')
            wash_boost.inputs['Brightness'].default_value = -0.03
            wash_boost.inputs['Contrast'].default_value = 32.0
            wash_tone = nodes.new(type='CompositorNodeCurveRGB')
            wash_tone.mapping.curves[3].points[0].location = (0.0, 0.08)
            wash_tone.mapping.curves[3].points[1].location = (1.0, 0.80)
            wash_tone.mapping.update()
            wash_band = nodes.new(type='CompositorNodePosterize')
            wash_band.inputs['Steps'].default_value = 8.0

            sobel = nodes.new(type='CompositorNodeFilter')
            sobel.inputs['Type'].default_value = 'Sobel'
            edge_blur = nodes.new(type='CompositorNodeBlur')
            edge_blur.inputs['Size'].default_value = (1.0, 1.0)
            edge_gray_2 = nodes.new(type='CompositorNodeRGBToBW')
            edge_norm = nodes.new(type='CompositorNodeNormalize')
            edge_shape = nodes.new(type='CompositorNodeCurveRGB')
            edge_shape.mapping.curves[3].points[0].location = (0.0, 0.0)
            edge_shape.mapping.curves[3].points[1].location = (1.0, 1.0)
            edge_shape.mapping.update()
            edge_boost = nodes.new(type='CompositorNodeBrightContrast')
            edge_boost.inputs['Brightness'].default_value = 0.0
            edge_boost.inputs['Contrast'].default_value = 58.0
            edge_to_alpha = nodes.new(type='CompositorNodeRGBToBW')
            ink_rgb = nodes.new(type='CompositorNodeRGB')
            ink_rgb.outputs['Color'].default_value = (0.05, 0.05, 0.05, 1.0)
            ink_overlay = nodes.new(type='CompositorNodeSetAlpha')
            ink_over = nodes.new(type='CompositorNodeAlphaOver')
            ink_over.inputs['Factor'].default_value = 0.96

            depth_norm = nodes.new(type='CompositorNodeNormalize')
            depth_shape = nodes.new(type='CompositorNodeCurveRGB')
            depth_shape.mapping.curves[3].points[0].location = (0.0, 0.0)
            depth_shape.mapping.curves[3].points[1].location = (1.0, 0.24)
            depth_shape.mapping.update()
            depth_to_alpha = nodes.new(type='CompositorNodeRGBToBW')
            fog_rgb = nodes.new(type='CompositorNodeRGB')
            fog_rgb.outputs['Color'].default_value = (0.88, 0.88, 0.88, 1.0)
            fog_overlay = nodes.new(type='CompositorNodeSetAlpha')
            fog_over = nodes.new(type='CompositorNodeAlphaOver')
            fog_over.inputs['Factor'].default_value = 0.05

            paper_img = nodes.new(type='CompositorNodeImage')
            paper_img.image = ensure_paper_noise_image()
            paper_to_alpha = nodes.new(type='CompositorNodeRGBToBW')
            paper_overlay = nodes.new(type='CompositorNodeSetAlpha')
            paper_rgb = nodes.new(type='CompositorNodeRGB')
            paper_rgb.outputs['Color'].default_value = (0.14, 0.14, 0.14, 1.0)
            paper_over = nodes.new(type='CompositorNodeAlphaOver')
            paper_over.inputs['Factor'].default_value = 0.03

            view = nodes.new(type='CompositorNodeViewer')

            links.new(rl.outputs['Image'], base_bw.inputs['Image'])
            links.new(base_bw.outputs['Val'], bw_to_img.inputs['Red'])
            links.new(base_bw.outputs['Val'], bw_to_img.inputs['Green'])
            links.new(base_bw.outputs['Val'], bw_to_img.inputs['Blue'])
            links.new(bw_to_img.outputs['Image'], wash_boost.inputs['Image'])
            links.new(wash_boost.outputs['Image'], wash_tone.inputs['Image'])
            links.new(wash_tone.outputs['Image'], wash_band.inputs['Image'])

            links.new(wash_boost.outputs['Image'], sobel.inputs['Image'])
            links.new(sobel.outputs['Image'], edge_blur.inputs['Image'])
            links.new(edge_blur.outputs['Image'], edge_gray_2.inputs['Image'])
            links.new(edge_gray_2.outputs['Val'], edge_norm.inputs['Value'])
            links.new(edge_norm.outputs['Value'], edge_shape.inputs['Image'])
            links.new(edge_shape.outputs['Image'], edge_boost.inputs['Image'])
            links.new(edge_boost.outputs['Image'], edge_to_alpha.inputs['Image'])
            links.new(ink_rgb.outputs['Color'], ink_overlay.inputs['Image'])
            links.new(edge_to_alpha.outputs['Val'], ink_overlay.inputs['Alpha'])

            links.new(wash_band.outputs['Image'], ink_over.inputs['Background'])
            links.new(ink_overlay.outputs['Image'], ink_over.inputs['Foreground'])

            links.new(rl.outputs['Depth'], depth_norm.inputs['Value'])
            links.new(depth_norm.outputs['Value'], depth_shape.inputs['Image'])
            links.new(depth_shape.outputs['Image'], depth_to_alpha.inputs['Image'])
            links.new(fog_rgb.outputs['Color'], fog_overlay.inputs['Image'])
            links.new(depth_to_alpha.outputs['Val'], fog_overlay.inputs['Alpha'])

            links.new(ink_over.outputs['Image'], fog_over.inputs['Background'])
            links.new(fog_overlay.outputs['Image'], fog_over.inputs['Foreground'])

            links.new(paper_img.outputs['Image'], paper_to_alpha.inputs['Image'])
            links.new(paper_rgb.outputs['Color'], paper_overlay.inputs['Image'])
            links.new(paper_to_alpha.outputs['Val'], paper_overlay.inputs['Alpha'])

            links.new(fog_over.outputs['Image'], paper_over.inputs['Background'])
            links.new(paper_overlay.outputs['Image'], paper_over.inputs['Foreground'])

            links.new(paper_over.outputs['Image'], view.inputs['Image'])
            final_img = paper_over.outputs['Image']

            if hasattr(scene, 'compositing_node_group') and scene.compositing_node_group == nt:
                existing_outputs = [
                    item for item in nt.interface.items_tree if getattr(item, 'in_out', None) == 'OUTPUT'
                ]
                has_image_output = any(item.name == 'Image' for item in existing_outputs)
                if not has_image_output:
                    nt.interface.new_socket(name='Image', in_out='OUTPUT', socket_type='NodeSocketColor')
                group_out = nodes.new(type='NodeGroupOutput')
                out_input = group_out.inputs.get('Image')
                if out_input is None and len(group_out.inputs) > 0:
                    out_input = group_out.inputs[0]
                if out_input is None:
                    raise RuntimeError('Compositor output socket not available')
                links.new(final_img, out_input)
            else:
                comp = nodes.new(type='CompositorNodeComposite')
                links.new(final_img, comp.inputs['Image'])

        def render_to(name, samples):
            set_render_defaults(samples)
            scene = bpy.context.scene
            scene.render.filepath = os.path.join(RENDER_DIR, name)
            bpy.ops.render.render(write_still=True)

        """
    )


def build_step_code(args: argparse.Namespace, blend_path: Path) -> str:
    hdr = code_header(
        args.mesh, args.out_dir, blend_path, args.width, args.height, args.samples, args.engine
    )

    if args.step == "1":
        body = """
        clear_compositor()
        bpy.ops.object.select_all(action='SELECT')
        bpy.ops.object.delete(use_global=False)

        for block in list(bpy.data.meshes):
            if block.users == 0:
                bpy.data.meshes.remove(block)
        for block in list(bpy.data.materials):
            if block.users == 0:
                bpy.data.materials.remove(block)

        existing = set(bpy.data.objects)
        try:
            bpy.ops.import_mesh.ply(filepath=MESH_PATH)
        except Exception:
            bpy.ops.wm.ply_import(filepath=MESH_PATH)

        new_objs = [o for o in bpy.data.objects if o not in existing and o.type == 'MESH']
        if new_objs:
            buddha = max(new_objs, key=lambda o: len(o.data.vertices))
        else:
            meshes = [o for o in bpy.data.objects if o.type == 'MESH']
            if not meshes:
                raise RuntimeError(f'No mesh imported from {MESH_PATH}')
            buddha = max(meshes, key=lambda o: len(o.data.vertices))

        buddha.name = 'Buddha'
        bpy.ops.object.select_all(action='DESELECT')
        buddha.select_set(True)
        bpy.context.view_layer.objects.active = buddha
        bpy.ops.object.shade_smooth()

        bpy.context.view_layer.update()
        corners = [buddha.matrix_world @ Vector(corner) for corner in buddha.bound_box]
        min_v = Vector((min(v.x for v in corners), min(v.y for v in corners), min(v.z for v in corners)))
        max_v = Vector((max(v.x for v in corners), max(v.y for v in corners), max(v.z for v in corners)))
        height = max_v.z - min_v.z
        if height > 1e-6:
            buddha.scale *= (1.72 / height)

        bpy.context.view_layer.update()
        corners = [buddha.matrix_world @ Vector(corner) for corner in buddha.bound_box]
        min_v = Vector((min(v.x for v in corners), min(v.y for v in corners), min(v.z for v in corners)))
        max_v = Vector((max(v.x for v in corners), max(v.y for v in corners), max(v.z for v in corners)))
        center_xy = Vector(((min_v.x + max_v.x) * 0.5, (min_v.y + max_v.y) * 0.5, 0.0))
        buddha.location.x -= center_xy.x
        buddha.location.y -= center_xy.y
        buddha.location.z -= min_v.z
        buddha.rotation_euler[2] = math.radians(14.0)

        buddha_mat = bpy.data.materials.new(name='BuddhaClay')
        buddha_mat.use_nodes = True
        bsdf = buddha_mat.node_tree.nodes.get('Principled BSDF')
        bsdf.inputs['Base Color'].default_value = (0.66, 0.64, 0.60, 1.0)
        bsdf.inputs['Roughness'].default_value = 0.82
        if buddha.data.materials:
            buddha.data.materials[0] = buddha_mat
        else:
            buddha.data.materials.append(buddha_mat)

        bpy.ops.mesh.primitive_plane_add(size=14.0, location=(0.0, 0.0, 0.0))
        ground = bpy.context.active_object
        ground.name = 'Ground'
        ground_mat = bpy.data.materials.new(name='GroundMat')
        ground_mat.use_nodes = True
        gbsdf = ground_mat.node_tree.nodes.get('Principled BSDF')
        gbsdf.inputs['Base Color'].default_value = (0.93, 0.92, 0.89, 1.0)
        gbsdf.inputs['Roughness'].default_value = 1.0
        ground.data.materials.append(ground_mat)

        cam_data = bpy.data.cameras.new(name='MainCameraData')
        cam_data.lens = 52
        cam = bpy.data.objects.new(name='MainCamera', object_data=cam_data)
        cam.location = (0.0, -3.0, 1.28)
        bpy.context.collection.objects.link(cam)
        look_at(cam, (0.0, 0.0, 0.92))
        bpy.context.scene.camera = cam
        bpy.context.scene.view_settings.view_transform = 'Standard'
        bpy.context.scene.view_settings.look = 'None'
        bpy.context.scene.view_settings.exposure = 0.0
        bpy.context.scene.view_settings.gamma = 1.0

        render_to('step1_geometry.png', max(48, SAMPLES // 2))
        bpy.ops.wm.save_as_mainfile(filepath=BLEND_PATH)
        print(json.dumps({'step': 1, 'render': os.path.join(RENDER_DIR, 'step1_geometry.png'), 'blend': BLEND_PATH}))
        """
    elif args.step == "2":
        body = """
        set_render_defaults(SAMPLES)
        clear_compositor()
        scene = bpy.context.scene
        scene.view_settings.view_transform = 'Filmic'
        scene.view_settings.look = 'Low Contrast'
        scene.view_settings.exposure = -0.80
        scene.view_settings.gamma = 1.0
        buddha = require_object('Buddha')
        cam = require_object('MainCamera')
        buddha_xf = capture_transform('Buddha')
        cam_xf = capture_transform('MainCamera')

        for light_name in ('SunMain', 'FillArea', 'RimLight', 'FaceKey'):
            obj = bpy.data.objects.get(light_name)
            if obj is not None:
                bpy.data.objects.remove(obj, do_unlink=True)
        remove_object_if_exists('BuddhaOcclusionShell')
        remove_object_if_exists('BuddhaRibbons')
        remove_object_if_exists('BuddhaGuideStrokes')

        world = bpy.data.worlds.get('BuddhaWorld')
        if world is None:
            world = bpy.data.worlds.new('BuddhaWorld')
        bpy.context.scene.world = world
        world.use_nodes = True
        wn = world.node_tree.nodes
        wl = world.node_tree.links
        wn.clear()

        world_out = wn.new(type='ShaderNodeOutputWorld')
        background = wn.new(type='ShaderNodeBackground')
        sky = wn.new(type='ShaderNodeTexSky')
        pick_sky_model(sky)
        if hasattr(sky, 'altitude'):
            sky.altitude = 1.0
        if hasattr(sky, 'air_density'):
            sky.air_density = 1.2
        # Keep sky node for step-3 sun animation controls, but use a neutral world backdrop.
        background.inputs['Color'].default_value = (0.88, 0.89, 0.90, 1.0)
        background.inputs['Strength'].default_value = 0.75
        wl.new(background.outputs['Background'], world_out.inputs['Surface'])

        sun_data = bpy.data.lights.new(name='SunMainData', type='SUN')
        sun_data.energy = 0.48
        sun_data.angle = math.radians(1.15)
        sun = bpy.data.objects.new(name='SunMain', object_data=sun_data)
        bpy.context.collection.objects.link(sun)
        set_sun_orientation(sun, sky, 168.0, 24.0)

        fill_data = bpy.data.lights.new(name='FillAreaData', type='AREA')
        fill_data.energy = 2.0
        fill_data.size = 3.2
        fill = bpy.data.objects.new(name='FillArea', object_data=fill_data)
        fill.location = (0.0, -3.3, 2.2)
        bpy.context.collection.objects.link(fill)
        look_at(fill, (0.0, 0.0, 0.9))

        rim_data = bpy.data.lights.new(name='RimData', type='SPOT')
        rim_data.energy = 3.2
        rim_data.spot_size = math.radians(58.0)
        rim = bpy.data.objects.new(name='RimLight', object_data=rim_data)
        rim.location = (1.9, 2.0, 2.3)
        bpy.context.collection.objects.link(rim)
        look_at(rim, (0.0, 0.0, 0.9))

        # Dedicated frontal face key to recover facial readability at static-camera framing.
        bpy.context.view_layer.update()
        corners = [buddha.matrix_world @ Vector(corner) for corner in buddha.bound_box]
        min_v = Vector((min(v.x for v in corners), min(v.y for v in corners), min(v.z for v in corners)))
        max_v = Vector((max(v.x for v in corners), max(v.y for v in corners), max(v.z for v in corners)))
        face_target = Vector((
            (min_v.x + max_v.x) * 0.5,
            (min_v.y + max_v.y) * 0.5,
            min_v.z + (max_v.z - min_v.z) * 0.80,
        ))
        to_face = (face_target - cam.location)
        if to_face.length > 1e-6:
            to_face.normalize()
        face_data = bpy.data.lights.new(name='FaceKeyData', type='AREA')
        face_data.energy = 7.5
        face_data.size = 1.1
        face_data.shape = 'SQUARE'
        face_key = bpy.data.objects.new(name='FaceKey', object_data=face_data)
        face_key.location = face_target - to_face * 1.45 + Vector((0.0, 0.0, 0.12))
        bpy.context.collection.objects.link(face_key)
        look_at(face_key, face_target)

        if buddha.data.materials and buddha.data.materials[0] is not None:
            mat = buddha.data.materials[0]
            mat.blend_method = 'OPAQUE'
            if mat.use_nodes:
                nt = mat.node_tree
                nodes = nt.nodes
                links = nt.links
                bsdf = next((n for n in mat.node_tree.nodes if n.bl_idname == 'ShaderNodeBsdfPrincipled'), None)
                if bsdf is not None:
                    bsdf.inputs['Roughness'].default_value = 0.82

                    # Rebuild base-color chain with cavity emphasis to reveal small sculpt detail.
                    for node_name in ('BuddhaBaseColor', 'BuddhaPointiness', 'BuddhaPointinessRamp', 'BuddhaCavityMix'):
                        old = nodes.get(node_name)
                        if old is not None:
                            nodes.remove(old)
                    for link in list(links):
                        if link.to_node == bsdf and link.to_socket == bsdf.inputs['Base Color']:
                            links.remove(link)

                    rgb = nodes.new(type='ShaderNodeRGB')
                    rgb.name = 'BuddhaBaseColor'
                    rgb.outputs['Color'].default_value = (0.45, 0.44, 0.42, 1.0)

                    geo = nodes.new(type='ShaderNodeNewGeometry')
                    geo.name = 'BuddhaPointiness'
                    ramp = nodes.new(type='ShaderNodeValToRGB')
                    ramp.name = 'BuddhaPointinessRamp'
                    ramp.color_ramp.interpolation = 'EASE'
                    ramp.color_ramp.elements[0].position = 0.30
                    ramp.color_ramp.elements[0].color = (0.82, 0.82, 0.82, 1.0)
                    ramp.color_ramp.elements[1].position = 0.72
                    ramp.color_ramp.elements[1].color = (1.02, 1.02, 1.02, 1.0)

                    mix = nodes.new(type='ShaderNodeMixRGB')
                    mix.name = 'BuddhaCavityMix'
                    mix.blend_type = 'MULTIPLY'
                    mix.inputs['Fac'].default_value = 0.34

                    links.new(geo.outputs['Pointiness'], ramp.inputs['Fac'])
                    links.new(rgb.outputs['Color'], mix.inputs['Color1'])
                    links.new(ramp.outputs['Color'], mix.inputs['Color2'])
                    links.new(mix.outputs['Color'], bsdf.inputs['Base Color'])

        ground = get_object('Ground')
        if ground is not None and ground.type == 'MESH' and ground.data.materials and ground.data.materials[0]:
            gmat = ground.data.materials[0]
            if gmat.use_nodes:
                gbsdf = next((n for n in gmat.node_tree.nodes if n.bl_idname == 'ShaderNodeBsdfPrincipled'), None)
                if gbsdf is not None:
                    gbsdf.inputs['Base Color'].default_value = (0.74, 0.74, 0.73, 1.0)
                    gbsdf.inputs['Roughness'].default_value = 1.0

        restore_transform('Buddha', buddha_xf)
        restore_transform('MainCamera', cam_xf)
        bpy.context.scene.camera = cam
        bpy.context.view_layer.update()

        render_to('step2_lighting_base.png', SAMPLES)
        bpy.ops.wm.save_as_mainfile(filepath=BLEND_PATH)
        print(json.dumps({'step': '2', 'render': os.path.join(RENDER_DIR, 'step2_lighting_base.png'), 'blend': BLEND_PATH}))
        """
    elif args.step == "2b":
        body = """
        set_render_defaults(SAMPLES)
        clear_compositor()
        scene = bpy.context.scene
        buddha = require_object('Buddha')
        cam = require_object('MainCamera')
        buddha_xf = capture_transform('Buddha')
        cam_xf = capture_transform('MainCamera')

        remove_object_if_exists('BuddhaOcclusionShell')
        remove_object_if_exists('BuddhaRibbons')
        remove_object_if_exists('BuddhaGuideStrokes')

        restore_transform('Buddha', buddha_xf)
        restore_transform('MainCamera', cam_xf)
        bpy.context.scene.camera = cam
        bpy.context.view_layer.update()

        render_to('step2b_no_occlusion.png', SAMPLES)
        bpy.ops.wm.save_as_mainfile(filepath=BLEND_PATH)
        print(json.dumps({'step': '2b', 'render': os.path.join(RENDER_DIR, 'step2b_no_occlusion.png'), 'blend': BLEND_PATH}))
        """
    elif args.step == "2c":
        body = """
        set_render_defaults(SAMPLES)
        clear_compositor()
        scene = bpy.context.scene
        buddha = require_object('Buddha')
        cam = require_object('MainCamera')
        buddha_xf = capture_transform('Buddha')
        cam_xf = capture_transform('MainCamera')

        remove_object_if_exists('BuddhaOcclusionShell')
        remove_object_if_exists('BuddhaGuideStrokes')
        ensure_ribbon_object(buddha)

        restore_transform('Buddha', buddha_xf)
        restore_transform('MainCamera', cam_xf)
        bpy.context.scene.camera = cam
        bpy.context.view_layer.update()

        render_to('step2c_ribbons.png', SAMPLES)
        bpy.ops.wm.save_as_mainfile(filepath=BLEND_PATH)
        print(json.dumps({'step': '2c', 'render': os.path.join(RENDER_DIR, 'step2c_ribbons.png'), 'blend': BLEND_PATH}))
        """
    elif args.step == "2d":
        body = """
        set_render_defaults(SAMPLES)
        scene = bpy.context.scene
        buddha = require_object('Buddha')
        cam = require_object('MainCamera')
        buddha_xf = capture_transform('Buddha')
        cam_xf = capture_transform('MainCamera')

        remove_object_if_exists('BuddhaOcclusionShell')
        remove_object_if_exists('BuddhaRibbons')
        remove_guide_strokes()
        setup_sumi_e_compositor(buddha)

        restore_transform('Buddha', buddha_xf)
        restore_transform('MainCamera', cam_xf)
        bpy.context.scene.camera = cam
        bpy.context.view_layer.update()

        render_to('step2d_sumi_e_compositor.png', SAMPLES)
        bpy.ops.wm.save_as_mainfile(filepath=BLEND_PATH)
        print(json.dumps({'step': '2d', 'render': os.path.join(RENDER_DIR, 'step2d_sumi_e_compositor.png'), 'blend': BLEND_PATH}))
        """
    else:
        body = """
        set_render_defaults(SAMPLES)
        clear_compositor()
        buddha_xf = capture_transform('Buddha')
        cam_xf = capture_transform('MainCamera')
        if buddha_xf is None or cam_xf is None:
            raise RuntimeError('Missing Buddha/MainCamera; run step 1 first or keep scene open')

        world = bpy.context.scene.world
        if world is None:
            raise RuntimeError('World not found; run step 2 first')
        world.use_nodes = True
        wn = world.node_tree.nodes
        wl = world.node_tree.links

        world_out = next((n for n in wn if n.bl_idname == 'ShaderNodeOutputWorld'), None)
        sky = next((n for n in wn if n.bl_idname == 'ShaderNodeTexSky'), None)
        background = next((n for n in wn if n.bl_idname == 'ShaderNodeBackground'), None)
        if world_out is None or sky is None or background is None:
            raise RuntimeError('Missing world nodes; run step 2 first')

        volume = next((n for n in wn if n.bl_idname == 'ShaderNodeVolumePrincipled'), None)
        if volume is None:
            volume = wn.new(type='ShaderNodeVolumePrincipled')
            volume.inputs['Density'].default_value = 0.004
            wl.new(volume.outputs['Volume'], world_out.inputs['Volume'])
        # World volume kills distant direct light (sun) quickly; disable for lighting direction checks.
        for link in list(wl):
            if link.to_node == world_out and link.to_socket == world_out.inputs['Volume']:
                wl.remove(link)
        volume.inputs['Density'].default_value = 0.0

        sun = get_object('SunMain')
        if sun is None:
            raise RuntimeError('SunMain not found; run step 2 first')
        cam = require_object('MainCamera')
        # Clear previous animation so preview renders use the just-set light direction.
        if sun.animation_data is not None:
            sun.animation_data_clear()
        if getattr(sky, 'id_data', None) is not None and sky.id_data.animation_data is not None:
            sky.id_data.animation_data_clear()
        if volume is not None and getattr(volume, 'id_data', None) is not None and volume.id_data.animation_data is not None:
            volume.id_data.animation_data_clear()
        scene = bpy.context.scene
        scene.view_settings.view_transform = 'Filmic'
        scene.view_settings.look = 'Low Contrast'
        scene.view_settings.gamma = 1.0
        scene.view_settings.exposure = -0.20

        sun.data.energy = 2.6
        sun.data.angle = math.radians(0.70)

        face_key = get_object('FaceKey')
        if face_key is not None and face_key.type == 'LIGHT':
            face_key.data.energy = 0.0
        fill = get_object('FillArea')
        if fill is not None and fill.type == 'LIGHT':
            fill.data.energy = 0.0
        rim = get_object('RimLight')
        if rim is not None and rim.type == 'LIGHT':
            rim.data.energy = 0.0
        background.inputs['Color'].default_value = (0.84, 0.85, 0.86, 1.0)
        background.inputs['Strength'].default_value = 0.60
        sun.data.energy = 5.5
        sun.data.angle = math.radians(0.25)

        def set_sun_camera_relative(cam_obj, side, elevation_deg):
            # side: -1.0 (camera-left) to +1.0 (camera-right)
            basis = cam_obj.matrix_world.to_3x3()
            cam_forward = -(basis @ Vector((0.0, 0.0, 1.0)))
            cam_right = basis @ Vector((1.0, 0.0, 0.0))
            cam_forward.normalize()
            cam_right.normalize()

            elev = math.radians(elevation_deg)
            horiz = max(math.cos(elev), 1e-5)
            direction = (
                cam_forward * (horiz * 0.88)
                + cam_right * (horiz * side * 0.62)
                + Vector((0.0, 0.0, math.sin(elev)))
            )
            if direction.length > 1e-6:
                direction.normalize()
            sun.rotation_euler = direction.to_track_quat('-Z', 'Y').to_euler()

            if hasattr(sky, 'sun_rotation'):
                sky.sun_rotation = math.atan2(direction.x, direction.y)
            if hasattr(sky, 'sun_elevation'):
                sky.sun_elevation = math.asin(max(-1.0, min(1.0, direction.z)))

        def render_variant(
            name,
            side,
            elev_deg,
            exposure,
            sun_energy,
            face_energy=0.0,
            fill_energy=0.0,
            rim_energy=0.0,
        ):
            set_sun_camera_relative(cam, side, elev_deg)
            sun.data.energy = sun_energy
            if face_key is not None and face_key.type == 'LIGHT':
                face_key.data.energy = face_energy
            if fill is not None and fill.type == 'LIGHT':
                fill.data.energy = fill_energy
            if rim is not None and rim.type == 'LIGHT':
                rim.data.energy = rim_energy
            scene.view_settings.exposure = exposure
            render_to(name, SAMPLES)

        render_variant('step3_morning.png', 3.2, 12.0, -0.65, 4.9, 0.0, 0.0, 0.0)
        render_variant('step3_midday.png', 0.20, 48.0, 0.28, 5.8, 1.55, 1.10, 0.55)
        render_variant('step3_evening.png', -3.2, 12.0, -0.65, 4.9, 0.0, 0.0, 0.0)

        restore_transform('Buddha', buddha_xf)
        restore_transform('MainCamera', cam_xf)
        cam = require_object('MainCamera')
        scene.camera = cam
        scene.frame_start = 1
        scene.frame_end = 120
        scene.frame_set(1)
        set_sun_camera_relative(cam, 3.2, 12.0)
        volume.inputs['Density'].default_value = 0.0
        sun.keyframe_insert(data_path='rotation_euler', frame=1)
        if hasattr(sky, 'sun_rotation'):
            sky.keyframe_insert(data_path='sun_rotation', frame=1)
        if hasattr(sky, 'sun_elevation'):
            sky.keyframe_insert(data_path='sun_elevation', frame=1)
        volume.inputs['Density'].keyframe_insert(data_path='default_value', frame=1)

        scene.frame_set(60)
        set_sun_camera_relative(cam, 0.0, 70.0)
        volume.inputs['Density'].default_value = 0.0
        sun.keyframe_insert(data_path='rotation_euler', frame=60)
        if hasattr(sky, 'sun_rotation'):
            sky.keyframe_insert(data_path='sun_rotation', frame=60)
        if hasattr(sky, 'sun_elevation'):
            sky.keyframe_insert(data_path='sun_elevation', frame=60)
        volume.inputs['Density'].keyframe_insert(data_path='default_value', frame=60)

        scene.frame_set(120)
        set_sun_camera_relative(cam, -3.2, 12.0)
        volume.inputs['Density'].default_value = 0.0
        sun.keyframe_insert(data_path='rotation_euler', frame=120)
        if hasattr(sky, 'sun_rotation'):
            sky.keyframe_insert(data_path='sun_rotation', frame=120)
        if hasattr(sky, 'sun_elevation'):
            sky.keyframe_insert(data_path='sun_elevation', frame=120)
        volume.inputs['Density'].keyframe_insert(data_path='default_value', frame=120)

        bpy.ops.wm.save_as_mainfile(filepath=BLEND_PATH)
        print(json.dumps({
            'step': 3,
            'renders': [
                os.path.join(RENDER_DIR, 'step3_morning.png'),
                os.path.join(RENDER_DIR, 'step3_midday.png'),
                os.path.join(RENDER_DIR, 'step3_evening.png'),
            ],
            'blend': BLEND_PATH,
            'animation_frames': [1, 60, 120],
        }))
        """

    return hdr + "\n" + textwrap.dedent(body)


def main() -> None:
    args = parse_args()
    if not args.mesh.exists():
        raise SystemExit(f"Mesh file not found: {args.mesh}")

    out_dir = args.out_dir.resolve()
    blend_path = out_dir / "blender" / "buddha_steps.blend"
    debug_dir = out_dir / "debug"
    debug_dir.mkdir(parents=True, exist_ok=True)

    code = build_step_code(args, blend_path)
    result = send_command(args.host, args.port, "execute_code", {"code": code}, timeout_s=args.timeout)

    scene_info = send_command(args.host, args.port, "get_scene_info", {}, timeout_s=30.0)
    (debug_dir / f"step{args.step}_scene_info.json").write_text(
        json.dumps(scene_info, indent=2), encoding="utf-8"
    )
    (debug_dir / f"step{args.step}_execute_result.json").write_text(
        json.dumps(result, indent=2), encoding="utf-8"
    )

    print(f"Completed step {args.step}. Output dir: {out_dir}")


if __name__ == "__main__":
    main()
