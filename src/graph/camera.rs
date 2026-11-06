//! Pure camera math for the graph view. The camera shows the world point `target` at the
//! screen centre: `screen = (world - target) * zoom + viewport / 2`, where `viewport` is the
//! window size in screen pixels.

use aberredengine::prelude::Vec2;

use crate::model::CamState;

/// Zoom limits for interactive zooming.
pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 4.0;

/// World point shown at screen pixel `p`.
pub fn screen_to_world(cam: &CamState, viewport: Vec2, p: Vec2) -> Vec2 {
    (p - viewport / 2.0) / cam.zoom + cam.target
}

/// Screen pixel at which world point `w` is shown (inverse of [`screen_to_world`]).
pub fn world_to_screen(cam: &CamState, viewport: Vec2, w: Vec2) -> Vec2 {
    (w - cam.target) * cam.zoom + viewport / 2.0
}

/// Zooms by `factor` around the screen cursor: the world point under `cursor_screen` stays
/// under it. The new zoom is clamped to `[MIN_ZOOM, MAX_ZOOM]`.
pub fn zoom_at(cam: &CamState, viewport: Vec2, cursor_screen: Vec2, factor: f32) -> CamState {
    let zoom = (cam.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    let world = screen_to_world(cam, viewport, cursor_screen);
    let target = world - (cursor_screen - viewport / 2.0) / zoom;
    CamState { target, zoom }
}

/// Pans by a screen-pixel delta: dragging the canvas by `delta_screen` moves the view by
/// `-delta_screen`, i.e. `target -= delta_screen / zoom`.
pub fn pan_by_screen_delta(cam: &CamState, delta_screen: Vec2) -> CamState {
    CamState {
        target: cam.target - delta_screen / cam.zoom,
        zoom: cam.zoom,
    }
}

/// Camera that fits `bounds` (world `(min, max)`) into the viewport with `margin_px` of padding
/// on every side, centred on the bounds. Zoom is clamped to `[MIN_ZOOM, 1.0]` so small graphs
/// are not magnified. `None` (empty doc) yields target `Vec2::ZERO`, zoom `1.0`.
pub fn frame_all(bounds: Option<(Vec2, Vec2)>, viewport: Vec2, margin_px: f32) -> CamState {
    let Some((min, max)) = bounds else {
        return CamState {
            target: Vec2::ZERO,
            zoom: 1.0,
        };
    };
    let centre = (min + max) / 2.0;
    let size = max - min;
    let avail = viewport - Vec2::splat(margin_px * 2.0);
    let zoom_x = if size.x > 0.0 {
        avail.x / size.x
    } else {
        f32::INFINITY
    };
    let zoom_y = if size.y > 0.0 {
        avail.y / size.y
    } else {
        f32::INFINITY
    };
    let zoom = zoom_x.min(zoom_y).clamp(MIN_ZOOM, 1.0);
    CamState {
        target: centre,
        zoom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        a.abs_diff_eq(b, 1e-4)
    }

    fn cam() -> CamState {
        CamState {
            target: Vec2::new(10.0, 20.0),
            zoom: 2.0,
        }
    }

    #[test]
    fn screen_world_round_trip() {
        let cam = cam();
        let viewport = Vec2::new(800.0, 600.0);
        for w in [
            Vec2::new(0.0, 0.0),
            Vec2::new(123.0, 456.0),
            Vec2::new(-50.0, 77.5),
        ] {
            let s = world_to_screen(&cam, viewport, w);
            assert!(
                close(screen_to_world(&cam, viewport, s), w),
                "round trip failed for {w:?}"
            );
        }
        // The camera target is shown at the screen centre.
        assert!(close(
            world_to_screen(&cam, viewport, cam.target),
            viewport / 2.0
        ));
    }

    #[test]
    fn zoom_at_keeps_the_world_point_under_the_cursor() {
        let cam = cam();
        let viewport = Vec2::new(800.0, 600.0);
        let cursor = Vec2::new(123.0, 456.0);
        let before = screen_to_world(&cam, viewport, cursor);
        let zoomed = zoom_at(&cam, viewport, cursor, 1.5);
        assert!((zoomed.zoom - 3.0).abs() < 1e-6);
        let after = screen_to_world(&zoomed, viewport, cursor);
        assert!(
            close(before, after),
            "the world point under the cursor must stay under it"
        );
        // Zoom clamps to [0.1, 4.0].
        assert!((zoom_at(&cam, viewport, cursor, 100.0).zoom - MAX_ZOOM).abs() < 1e-6);
        assert!((zoom_at(&cam, viewport, cursor, 0.0001).zoom - MIN_ZOOM).abs() < 1e-6);
    }

    #[test]
    fn pan_by_screen_delta_divides_by_zoom() {
        let cam = cam();
        let panned = pan_by_screen_delta(&cam, Vec2::new(100.0, -40.0));
        assert!(close(
            panned.target,
            cam.target - Vec2::new(100.0, -40.0) / cam.zoom
        ));
        assert!((panned.zoom - cam.zoom).abs() < 1e-6);
    }

    #[test]
    fn frame_all_contains_the_bounds_and_none_is_default() {
        let viewport = Vec2::new(800.0, 600.0);
        let margin = 40.0;
        let bounds = (Vec2::new(0.0, 0.0), Vec2::new(2000.0, 1000.0));
        let cam = frame_all(Some(bounds), viewport, margin);
        assert!(
            close(cam.target, Vec2::new(1000.0, 500.0)),
            "centred on the bounds"
        );
        // Every corner of the bounds maps inside the padded viewport.
        for corner in [
            bounds.0,
            bounds.1,
            Vec2::new(bounds.0.x, bounds.1.y),
            Vec2::new(bounds.1.x, bounds.0.y),
        ] {
            let s = world_to_screen(&cam, viewport, corner);
            assert!(
                s.x >= margin - 1e-3
                    && s.x <= viewport.x - margin + 1e-3
                    && s.y >= margin - 1e-3
                    && s.y <= viewport.y - margin + 1e-3,
                "corner {corner:?} maps to {s:?}, outside the padded viewport"
            );
        }
        // Small graphs are not magnified: zoom clamps to 1.0.
        let small = frame_all(
            Some((Vec2::ZERO, Vec2::new(100.0, 100.0))),
            viewport,
            margin,
        );
        assert!((small.zoom - 1.0).abs() < 1e-6);
        // No bounds: target ZERO, zoom 1.0.
        let none = frame_all(None, viewport, margin);
        assert!(close(none.target, Vec2::ZERO));
        assert!((none.zoom - 1.0).abs() < 1e-6);
    }
}
