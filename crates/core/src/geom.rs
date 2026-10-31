//! A page's four corners as a [`Quad`], and the geometry cleaning, reframing and the corner editor
//! share.

/// A page's corners in photo pixels: top-left, top-right, bottom-right, bottom-left.
pub type Quad = [[f32; 2]; 4];

/// Sharpest corner a page may have, in degrees; its flattest is 180° less this. Past these,
/// straightening stretches part of the page towards infinity, and a dent (over 180°) isn't a page.
const MIN_CORNER_DEG: f32 = 15.0;

/// The corners of a `width` x `height` image, in [`Quad`] order: cropping to them only scales it.
pub fn rect_corners(width: u32, height: u32) -> Quad {
    let (w, h) = (width as f32, height as f32);
    [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]
}

/// The average of `quad`'s corners.
pub fn centroid(quad: &Quad) -> [f32; 2] {
    let sum = |axis: usize| quad.iter().map(|p| p[axis]).sum::<f32>() / 4.0;
    [sum(0), sum(1)]
}

pub fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Orders corners top-left, top-right, bottom-right, bottom-left. Sorted by angle around their
/// centre, so the sides between them never cross, whatever order they came in.
pub fn order_corners(mut corners: Quad) -> Quad {
    let [cx, cy] = centroid(&corners);
    // With y pointing down, increasing angle runs clockwise on screen.
    let angle = |p: &[f32; 2]| (p[1] - cy).atan2(p[0] - cx);
    corners.sort_by(|a, b| angle(a).total_cmp(&angle(b)));
    let top_left = (0..4)
        .min_by(|&i, &j| {
            let sum = |p: [f32; 2]| p[0] + p[1];
            sum(corners[i]).total_cmp(&sum(corners[j]))
        })
        .unwrap_or(0);
    corners.rotate_left(top_left);
    corners
}

/// Whether `quad`, in [`Quad`] order, can be straightened: it's convex, clockwise on screen, and
/// every corner is between [`MIN_CORNER_DEG`] and 180° less that.
pub fn is_page_shaped(quad: &Quad) -> bool {
    let (min, max) = (
        MIN_CORNER_DEG.to_radians(),
        (180.0 - MIN_CORNER_DEG).to_radians(),
    );
    (0..4).all(|i| {
        let [px, py] = quad[(i + 3) % 4];
        let [x, y] = quad[i];
        let [nx, ny] = quad[(i + 1) % 4];
        let (a, b) = ([px - x, py - y], [nx - x, ny - y]);
        // With y pointing down, the next corner is clockwise of the previous one: a right turn.
        let cross = a[0] * b[1] - a[1] * b[0];
        let angle = cross.abs().atan2(a[0] * b[0] + a[1] * b[1]);
        cross < 0.0 && (min..=max).contains(&angle)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUAD: Quad = [
        [200.0, 120.0],
        [700.0, 90.0],
        [740.0, 800.0],
        [150.0, 760.0],
    ];

    #[test]
    fn rect_corners_go_clockwise_from_the_origin() {
        assert_eq!(
            rect_corners(300, 200),
            [[0.0, 0.0], [300.0, 0.0], [300.0, 200.0], [0.0, 200.0]]
        );
        assert_eq!(centroid(&rect_corners(300, 200)), [150.0, 100.0]);
        assert_eq!(dist([0.0, 0.0], [3.0, 4.0]), 5.0);
    }

    #[test]
    fn orders_corners_clockwise_from_top_left() {
        let shuffled = [
            [700.0, 90.0],
            [150.0, 760.0],
            [200.0, 120.0],
            [740.0, 800.0],
        ];
        assert_eq!(order_corners(shuffled), QUAD);
    }

    /// Whether segments `a`-`b` and `c`-`d` cross.
    fn cross(a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]) -> bool {
        let side = |p: [f32; 2], q: [f32; 2], r: [f32; 2]| {
            ((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])).signum()
        };
        side(a, b, c) != side(a, b, d) && side(c, d, a) != side(c, d, b)
    }

    #[test]
    fn ordered_corners_never_cross() {
        let concave = [[0.0, 0.0], [100.0, 0.0], [50.0, 30.0], [50.0, 100.0]];
        let skewed = [[10.0, 50.0], [90.0, 0.0], [100.0, 60.0], [0.0, 100.0]];
        for points in [QUAD, concave, skewed] {
            // Every order the user could click them in.
            for perm in 0..24 {
                let mut rest: Vec<[f32; 2]> = points.to_vec();
                let mut shuffled = [[0.0; 2]; 4];
                let mut n = perm;
                for (i, slot) in shuffled.iter_mut().enumerate() {
                    *slot = rest.remove(n % (4 - i));
                    n /= 4 - i;
                }
                let q = order_corners(shuffled);
                assert!(
                    !cross(q[0], q[1], q[2], q[3]) && !cross(q[1], q[2], q[3], q[0]),
                    "{shuffled:?} ordered to {q:?}"
                );
            }
        }
    }

    #[test]
    fn page_shaped_quads_are_convex_without_extreme_corners() {
        assert!(is_page_shaped(&QUAD));
        assert!(is_page_shaped(&rect_corners(300, 200)));
        let not = [
            (
                "dented",
                [[0.0, 0.0], [100.0, 0.0], [50.0, 30.0], [50.0, 100.0]],
            ),
            (
                "anticlockwise",
                [[0.0, 0.0], [0.0, 100.0], [100.0, 100.0], [100.0, 0.0]],
            ),
            (
                "crossed",
                [[0.0, 0.0], [100.0, 100.0], [100.0, 0.0], [0.0, 100.0]],
            ),
            (
                "flat corner",
                [[0.0, 0.0], [50.0, 2.0], [100.0, 0.0], [50.0, 100.0]],
            ),
            (
                "sharp corner",
                [[0.0, 0.0], [300.0, 45.0], [50.0, 60.0], [0.0, 50.0]],
            ),
            (
                "two corners together",
                [[0.0, 0.0], [0.0, 0.0], [100.0, 100.0], [0.0, 100.0]],
            ),
        ];
        for (name, quad) in not {
            assert!(!is_page_shaped(&quad), "{name}");
        }
    }
}
