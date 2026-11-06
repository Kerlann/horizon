//! Fixed-capacity output for a view's custom geometry; the GPU consumer owns drawing.
use crate::gfx::world_primitives::{Batch, DEFAULT_CAPACITY as CAPACITY};
use crate::{gizmos, mesher::Vertex};
use glam::Mat4;
use mod_api::{DrawError, WorldDepth, WorldDrawUsage, WorldLine, WorldQuad};

/// Reused between views, including frames with no subscribers or output.
pub struct WorldGeometry {
    camera: [f64; 3],
    view: Mat4,
    white: [f32; 2],
    lines: [Vec<[f32; 8]>; 2],
    quads: [Vec<Vertex>; 2],
}
impl Default for WorldGeometry {
    fn default() -> Self {
        Self {
            camera: [0.0; 3],
            view: Mat4::IDENTITY,
            white: [0.0; 2],
            lines: std::array::from_fn(|_| Vec::with_capacity(CAPACITY as usize)),
            quads: std::array::from_fn(|_| Vec::with_capacity(CAPACITY as usize * 4)),
        }
    }
}
impl WorldGeometry {
    /// Start a view even if nothing will be submitted; previous output disappears.
    pub fn reset(&mut self, camera: [f64; 3], view: Mat4, white: [f32; 2]) {
        self.camera = camera;
        self.view = view;
        self.white = white;
        self.lines.iter_mut().for_each(Vec::clear);
        self.quads.iter_mut().for_each(Vec::clear);
    }
    /// The current counts and setup-time limits across both depth modes.
    pub fn usage(&self) -> WorldDrawUsage {
        WorldDrawUsage::new(
            self.lines.iter().map(|v| v.len() as u32).sum(),
            CAPACITY,
            self.quads.iter().map(|v| v.len() as u32 / 4).sum(),
            CAPACITY,
        )
    }
    /// Validate once at the submission boundary, before any output is appended.
    fn validate_positions(&self, points: impl Iterator<Item = [f64; 3]>) -> Result<(), DrawError> {
        if points
            .flatten()
            .enumerate()
            .any(|(i, p)| !p.is_finite() || !((p - self.camera[i % 3]) as f32).is_finite())
        {
            return Err(DrawError::NonFinite);
        }
        Ok(())
    }
    /// Append an entire line batch or leave the output unchanged.
    pub fn lines(&mut self, lines: &[WorldLine]) -> Result<(), DrawError> {
        if lines.len() > (CAPACITY - self.usage().lines) as usize {
            return Err(DrawError::CapacityExceeded);
        }
        self.validate_positions(lines.iter().flat_map(|l| [l.from, l.to]))?;
        for l in lines {
            if !l.width.is_finite() {
                return Err(DrawError::NonFinite);
            }
            if l.width <= 0.0 {
                return Err(DrawError::InvalidWidth);
            }
        }
        for l in lines {
            let line = gizmos::Line {
                start: l.from,
                end: l.to,
                color: l.color,
                width: l.width,
            };
            let out = &mut self.lines[usize::from(l.depth == WorldDepth::ThroughWalls)];
            gizmos::camera_relative_to(&[line], self.camera, self.view, |line| out.push(line));
        }
        Ok(())
    }
    /// Append an entire quad batch or leave the output unchanged.
    pub fn quads(&mut self, quads: &[WorldQuad]) -> Result<(), DrawError> {
        if quads.len() > (CAPACITY - self.usage().quads) as usize {
            return Err(DrawError::CapacityExceeded);
        }
        self.validate_positions(quads.iter().flat_map(|q| q.corners))?;
        for q in quads {
            let color = crate::world_primitives::vertex_color(q.color, 255);
            self.quads[usize::from(q.depth == WorldDepth::ThroughWalls)].extend(q.corners.map(
                |p| Vertex {
                    pos: std::array::from_fn(|i| (p[i] - self.camera[i]) as f32),
                    uv: self.white,
                    color,
                    layer: 0,
                    light: 0,
                    normal: [0; 4],
                    glint_uv: [0.0; 2],
                },
            ));
        }
        Ok(())
    }
    /// Borrow the existing renderer's batch, with no copying or frame allocation.
    pub fn batch(&self) -> Batch<'_> {
        Batch {
            lines: &self.lines[0],
            lines_through: &self.lines[1],
            quads: &self.quads[0],
            quads_through: &self.quads[1],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn world_geometry_batches_are_atomic_and_reset_per_view() {
        let mut draw = WorldGeometry::default();
        draw.reset([30_000_000.0, 20.0, 0.0], Mat4::IDENTITY, [0.25, 0.75]);
        let line = WorldLine::new(
            [30_000_000.25, 20.0, -1.0],
            [30_000_001.0, 20.0, -1.0],
            0x80402010,
            2.0,
            WorldDepth::Tested,
        );
        draw.lines(&[line]).unwrap();
        assert_eq!(
            &draw.batch().lines[0][..6],
            &[0.25, 0.0, -1.0, 1.0, 0.0, -1.0]
        );
        assert_eq!(draw.batch().lines[0][6].to_bits(), line.color);
        let mut bad = line;
        bad.width = 0.0;
        assert_eq!(draw.lines(&[line, bad]), Err(DrawError::InvalidWidth));
        bad.width = f32::NAN;
        assert_eq!(draw.lines(&[line, bad]), Err(DrawError::NonFinite));
        bad = line;
        bad.to[0] = f64::MAX;
        assert_eq!(draw.lines(&[line, bad]), Err(DrawError::NonFinite));
        assert_eq!(draw.usage().lines, 1);
        let quad = WorldQuad::new([line.from; 4], line.color, WorldDepth::ThroughWalls);
        draw.quads(&[quad]).unwrap();
        assert_eq!(draw.batch().quads_through.len(), 4);
        assert_eq!(draw.batch().quads_through[0].pos, [0.25, 0.0, -1.0]);
        assert_eq!(
            draw.batch().quads_through[0].color.to_le_bytes(),
            [0x40, 0x20, 0x10, 0x80]
        );
        let mut invalid_quad = quad;
        invalid_quad.corners[3][1] = f64::INFINITY;
        assert_eq!(draw.quads(&[quad, invalid_quad]), Err(DrawError::NonFinite));
        assert_eq!(draw.usage().quads, 1);
        let mut through = line;
        through.depth = WorldDepth::ThroughWalls;
        draw.lines(&vec![through; CAPACITY as usize - 1]).unwrap();
        assert_eq!(draw.lines(&[line]), Err(DrawError::CapacityExceeded));
        draw.quads(&vec![quad; CAPACITY as usize - 1]).unwrap();
        assert_eq!(draw.quads(&[quad]), Err(DrawError::CapacityExceeded));
        let capacities = (
            draw.lines.each_ref().map(Vec::capacity),
            draw.quads.each_ref().map(Vec::capacity),
        );
        draw.reset([0.0; 3], Mat4::IDENTITY, [0.0; 2]);
        assert!(draw.batch().is_empty());
        assert_eq!(
            capacities,
            (
                draw.lines.each_ref().map(Vec::capacity),
                draw.quads.each_ref().map(Vec::capacity)
            )
        );
    }
}
