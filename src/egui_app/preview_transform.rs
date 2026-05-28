use eframe::egui::{self, Color32, Pos2, Rect, TextureId, Ui, Vec2};
use uuid::Uuid;

use crate::core::preview::PreviewLayerStack;
use crate::state::{Clip, ClipTransform, Project};

use super::PREVIEW_ROTATE_HANDLE_DISTANCE;

#[derive(Clone, Copy, Debug)]
pub(super) enum PreviewTransformDrag {
    Pan {
        start_pan: Vec2,
        start_pointer: Pos2,
    },
    Move {
        clip_id: Uuid,
        start_transform: ClipTransform,
        start_pointer_project: Pos2,
        start_half_size: Vec2,
    },
    Scale {
        clip_id: Uuid,
        handle: PreviewScaleHandle,
        start_transform: ClipTransform,
        start_center_project: Pos2,
        start_half_size: Vec2,
    },
    Rotate {
        clip_id: Uuid,
        start_transform: ClipTransform,
        start_center_project: Pos2,
        start_pointer_angle: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreviewScaleHandle {
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreviewScaleSnapAxis {
    X,
    Y,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PreviewSnapGuide {
    pub(super) start: Pos2,
    pub(super) end: Pos2,
}

#[derive(Clone, Debug)]
pub(super) struct PreviewObjectGeometry {
    pub(super) clip_id: Uuid,
    pub(super) project_rect: Rect,
    pub(super) screen_corners: [Pos2; 4],
    pub(super) screen_center: Pos2,
    pub(super) project_to_screen: f32,
}

pub(super) fn preview_fit_scale(rect: Rect, layers: &PreviewLayerStack) -> f32 {
    (rect.width() / layers.canvas_width.max(1) as f32)
        .min(rect.height() / layers.canvas_height.max(1) as f32)
        .max(0.01)
}

pub(super) fn preview_project_scale(layers: &PreviewLayerStack, project_width: u32) -> f32 {
    layers.canvas_width.max(1) as f32 / project_width.max(1) as f32
}

pub(super) fn preview_geometry_for_clip(
    project: &Project,
    clip: &Clip,
    source_size: Vec2,
    canvas_rect: Rect,
    layers: &PreviewLayerStack,
    canvas_scale: f32,
) -> PreviewObjectGeometry {
    let project_w = project.settings.width.max(1) as f32;
    let project_h = project.settings.height.max(1) as f32;
    let preview_scale = preview_project_scale(layers, project.settings.width);
    let project_to_screen = (preview_scale * canvas_scale).max(0.0001);
    let project_center = Pos2::new(project_w * 0.5, project_h * 0.5);
    let half_size = Vec2::new(
        source_size.x.max(1.0) * clip.transform.scale_x.max(0.01) * 0.5,
        source_size.y.max(1.0) * clip.transform.scale_y.max(0.01) * 0.5,
    );
    let center = project_center + Vec2::new(clip.transform.position_x, clip.transform.position_y);
    let project_rect = Rect::from_center_size(center, half_size * 2.0);
    let corners_project = [
        project_rect.left_top(),
        project_rect.right_top(),
        project_rect.right_bottom(),
        project_rect.left_bottom(),
    ];
    let screen_corners = corners_project.map(|point| {
        let rotated = rotate_point(point, center, clip.transform.rotation_deg);
        preview_project_to_screen(rotated, canvas_rect, preview_scale, canvas_scale)
    });

    PreviewObjectGeometry {
        clip_id: clip.id,
        project_rect,
        screen_corners,
        screen_center: preview_project_to_screen(center, canvas_rect, preview_scale, canvas_scale),
        project_to_screen,
    }
}

pub(super) fn preview_scroll_delta(ui: &Ui, rect: Rect) -> f32 {
    let pointer_in_rect = ui
        .ctx()
        .pointer_hover_pos()
        .map(|pointer| rect.contains(pointer))
        .unwrap_or(false);
    if !pointer_in_rect {
        return 0.0;
    }
    ui.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel { delta, .. } => Some(delta.y),
                _ => None,
            })
            .sum()
    })
}

pub(super) fn preview_project_to_screen(
    point: Pos2,
    canvas_rect: Rect,
    preview_scale: f32,
    canvas_scale: f32,
) -> Pos2 {
    canvas_rect.min + Vec2::new(point.x, point.y) * preview_scale * canvas_scale
}

pub(super) fn preview_screen_to_project(
    point: Pos2,
    canvas_rect: Rect,
    layers: &PreviewLayerStack,
    project_width: u32,
) -> Pos2 {
    let preview_scale = preview_project_scale(layers, project_width);
    let canvas_scale = (canvas_rect.width() / layers.canvas_width.max(1) as f32).max(0.0001);
    let project = (point - canvas_rect.min) / (preview_scale * canvas_scale).max(0.0001);
    Pos2::new(project.x, project.y)
}

pub(super) fn rotate_point(point: Pos2, center: Pos2, degrees: f32) -> Pos2 {
    center + rotate_vec(point - center, degrees)
}

pub(super) fn rotate_vec(vec: Vec2, degrees: f32) -> Vec2 {
    let radians = degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    Vec2::new(vec.x * cos - vec.y * sin, vec.x * sin + vec.y * cos)
}

pub(super) fn vector_angle_deg(vec: Vec2) -> f32 {
    vec.y.atan2(vec.x).to_degrees()
}

pub(super) fn rect_from_points(points: &[Pos2]) -> Rect {
    let mut min = Pos2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for point in points {
        min.x = min.x.min(point.x);
        min.y = min.y.min(point.y);
        max.x = max.x.max(point.x);
        max.y = max.y.max(point.y);
    }
    Rect::from_min_max(min, max)
}

pub(super) fn preview_scale_handle_points(
    object: &PreviewObjectGeometry,
) -> [(PreviewScaleHandle, Pos2); 8] {
    let [nw, ne, se, sw] = object.screen_corners;
    [
        (PreviewScaleHandle::NorthWest, nw),
        (
            PreviewScaleHandle::North,
            Pos2::new((nw.x + ne.x) * 0.5, (nw.y + ne.y) * 0.5),
        ),
        (PreviewScaleHandle::NorthEast, ne),
        (
            PreviewScaleHandle::East,
            Pos2::new((ne.x + se.x) * 0.5, (ne.y + se.y) * 0.5),
        ),
        (PreviewScaleHandle::SouthEast, se),
        (
            PreviewScaleHandle::South,
            Pos2::new((se.x + sw.x) * 0.5, (se.y + sw.y) * 0.5),
        ),
        (PreviewScaleHandle::SouthWest, sw),
        (
            PreviewScaleHandle::West,
            Pos2::new((sw.x + nw.x) * 0.5, (sw.y + nw.y) * 0.5),
        ),
    ]
}

pub(super) fn preview_rotate_handle_point(object: &PreviewObjectGeometry) -> Pos2 {
    let top_mid = preview_scale_handle_points(object)
        .iter()
        .find(|(handle, _)| *handle == PreviewScaleHandle::North)
        .map(|(_, point)| *point)
        .unwrap_or(object.screen_center);
    let offset = top_mid - object.screen_center;
    let direction = if offset.length_sq() > 0.0001 {
        offset.normalized()
    } else {
        Vec2::new(0.0, -1.0)
    };
    top_mid + direction * PREVIEW_ROTATE_HANDLE_DISTANCE
}

pub(super) fn preview_scaled_transform(
    start_transform: ClipTransform,
    start_center_project: Pos2,
    pointer_project: Pos2,
    handle: PreviewScaleHandle,
    start_half_size: Vec2,
    constrain_aspect: bool,
    snap_axis: Option<PreviewScaleSnapAxis>,
    project_to_screen: f32,
) -> ClipTransform {
    let (sx, sy) = preview_scale_handle_signs(handle);
    let min_half = (4.0 / project_to_screen.max(0.0001)).max(0.5);
    let start_half_x = start_half_size.x.max(min_half);
    let start_half_y = start_half_size.y.max(min_half);
    let pointer_local = rotate_vec(
        pointer_project - start_center_project,
        -start_transform.rotation_deg,
    );

    let mut new_half_x = start_half_x;
    let mut new_half_y = start_half_y;
    let mut center_local = Vec2::ZERO;

    if sx != 0.0 {
        let anchor_x = -sx * start_half_x;
        let handle_x = pointer_local.x;
        new_half_x = ((handle_x - anchor_x).abs() * 0.5).max(min_half);
        center_local.x = (handle_x + anchor_x) * 0.5;
    }
    if sy != 0.0 {
        let anchor_y = -sy * start_half_y;
        let handle_y = pointer_local.y;
        new_half_y = ((handle_y - anchor_y).abs() * 0.5).max(min_half);
        center_local.y = (handle_y + anchor_y) * 0.5;
    }

    if constrain_aspect && (sx != 0.0 || sy != 0.0) {
        let factor_x = if sx != 0.0 {
            new_half_x / start_half_x.max(0.0001)
        } else {
            1.0
        };
        let factor_y = if sy != 0.0 {
            new_half_y / start_half_y.max(0.0001)
        } else {
            1.0
        };
        let mut factor = match (sx != 0.0, sy != 0.0, snap_axis) {
            (true, true, Some(PreviewScaleSnapAxis::X)) => factor_x,
            (true, true, Some(PreviewScaleSnapAxis::Y)) => factor_y,
            (true, true, None) => factor_x.max(factor_y),
            (true, false, _) => factor_x,
            (false, true, _) => factor_y,
            _ => 1.0,
        };
        factor = factor
            .max(min_half / start_half_x.max(0.0001))
            .max(min_half / start_half_y.max(0.0001));
        new_half_x = start_half_x * factor;
        new_half_y = start_half_y * factor;

        center_local = Vec2::ZERO;
        if sx != 0.0 {
            let anchor_x = -sx * start_half_x;
            center_local.x = anchor_x + sx * new_half_x;
        }
        if sy != 0.0 {
            let anchor_y = -sy * start_half_y;
            center_local.y = anchor_y + sy * new_half_y;
        }
    }

    let start_project_origin = Pos2::new(
        start_center_project.x - start_transform.position_x,
        start_center_project.y - start_transform.position_y,
    );
    let next_center = start_center_project + rotate_vec(center_local, start_transform.rotation_deg);
    let mut transform = start_transform;
    transform.position_x = next_center.x - start_project_origin.x;
    transform.position_y = next_center.y - start_project_origin.y;
    if start_half_x > 0.0 {
        transform.scale_x = start_transform.scale_x * (new_half_x / start_half_x);
    }
    if start_half_y > 0.0 {
        transform.scale_y = start_transform.scale_y * (new_half_y / start_half_y);
    }
    transform
}

pub(super) fn preview_scale_cursor(handle: PreviewScaleHandle) -> egui::CursorIcon {
    match handle {
        PreviewScaleHandle::North | PreviewScaleHandle::South => egui::CursorIcon::ResizeVertical,
        PreviewScaleHandle::East | PreviewScaleHandle::West => egui::CursorIcon::ResizeHorizontal,
        PreviewScaleHandle::NorthEast | PreviewScaleHandle::SouthWest => {
            egui::CursorIcon::ResizeNeSw
        }
        PreviewScaleHandle::NorthWest | PreviewScaleHandle::SouthEast => {
            egui::CursorIcon::ResizeNwSe
        }
    }
}

pub(super) fn preview_scale_handle_signs(handle: PreviewScaleHandle) -> (f32, f32) {
    match handle {
        PreviewScaleHandle::NorthWest => (-1.0, -1.0),
        PreviewScaleHandle::North => (0.0, -1.0),
        PreviewScaleHandle::NorthEast => (1.0, -1.0),
        PreviewScaleHandle::East => (1.0, 0.0),
        PreviewScaleHandle::SouthEast => (1.0, 1.0),
        PreviewScaleHandle::South => (0.0, 1.0),
        PreviewScaleHandle::SouthWest => (-1.0, 1.0),
        PreviewScaleHandle::West => (-1.0, 0.0),
    }
}

pub(super) fn nearest_snap_delta<const N: usize>(
    candidates: [f32; N],
    targets: &[f32],
    threshold: f32,
) -> Option<(f32, f32)> {
    let mut best: Option<(f32, f32, f32)> = None;
    for candidate in candidates {
        for target in targets {
            let delta = *target - candidate;
            let distance = delta.abs();
            if distance <= threshold
                && best
                    .map(|(_, _, best_distance)| distance < best_distance)
                    .unwrap_or(true)
            {
                best = Some((delta, *target, distance));
            }
        }
    }
    best.map(|(delta, target, _)| (delta, target))
}

pub(super) fn paint_rotated_texture(
    painter: &egui::Painter,
    texture_id: TextureId,
    rect: Rect,
    rotation_deg: f32,
    color: Color32,
) {
    let center = rect.center();
    let radians = rotation_deg.to_radians();
    let (sin, cos) = radians.sin_cos();
    let rotate = |pos: Pos2| {
        let offset = pos - center;
        Pos2::new(
            center.x + offset.x * cos - offset.y * sin,
            center.y + offset.x * sin + offset.y * cos,
        )
    };

    let corners = [
        rotate(rect.left_top()),
        rotate(rect.right_top()),
        rotate(rect.right_bottom()),
        rotate(rect.left_bottom()),
    ];
    let uvs = [
        Pos2::new(0.0, 0.0),
        Pos2::new(1.0, 0.0),
        Pos2::new(1.0, 1.0),
        Pos2::new(0.0, 1.0),
    ];

    let mut mesh = egui::epaint::Mesh::with_texture(texture_id);
    let base = mesh.vertices.len() as u32;
    for index in 0..4 {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: corners[index],
            uv: uvs[index],
            color,
        });
    }
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    painter.add(egui::Shape::mesh(mesh));
}
