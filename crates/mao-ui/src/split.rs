//! Binary split tree of panes inside a tab. See `docs/specs/workspace-layout.md`.

use crate::geom::Rect;

/// Identifies a pane across the whole workspace.
pub type PaneId = u64;

/// Where a new pane goes relative to the split one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDir {
    /// Side by side (the new pane on the right).
    Right,
    /// Stacked (the new pane below).
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Leaf(PaneId),
    Split {
        dir: SplitDir,
        /// Fraction of the space given to `a` (0..1).
        ratio: f32,
        a: Box<Node>,
        b: Box<Node>,
    },
}

/// Path from the root to a split node: `false` = first child, `true` = second.
pub type NodePath = Vec<bool>;

/// A draggable boundary between the two children of a split.
#[derive(Debug, Clone, PartialEq)]
pub struct Divider {
    pub path: NodePath,
    pub dir: SplitDir,
    /// The thin rectangle between the children (`gap` wide).
    pub rect: Rect,
    /// Area covered by the whole split node (to turn a position into a ratio).
    pub area: Rect,
}

const MIN_RATIO: f32 = 0.05;

#[derive(Debug, Clone, PartialEq)]
pub struct SplitTree {
    root: Node,
}

impl SplitTree {
    pub fn new(pane: PaneId) -> Self {
        Self {
            root: Node::Leaf(pane),
        }
    }

    /// Splits `target` in half, placing `new` to its right or below.
    /// Returns `false` if `target` is not in the tree.
    pub fn split(&mut self, target: PaneId, dir: SplitDir, new: PaneId) -> bool {
        fn go(node: &mut Node, target: PaneId, dir: SplitDir, new: PaneId) -> bool {
            match node {
                Node::Leaf(id) if *id == target => {
                    *node = Node::Split {
                        dir,
                        ratio: 0.5,
                        a: Box::new(Node::Leaf(target)),
                        b: Box::new(Node::Leaf(new)),
                    };
                    true
                }
                Node::Leaf(_) => false,
                Node::Split { a, b, .. } => go(a, target, dir, new) || go(b, target, dir, new),
            }
        }
        go(&mut self.root, target, dir, new)
    }

    /// Removes `target`, giving its space to its sibling.
    /// Returns `false` if `target` is the last pane (the caller closes the tab) or absent.
    pub fn remove(&mut self, target: PaneId) -> bool {
        fn go(node: &mut Node, target: PaneId) -> bool {
            let Node::Split { a, b, .. } = node else {
                return false;
            };
            let sibling = if matches!(**a, Node::Leaf(id) if id == target) {
                Some(std::mem::replace(&mut **b, Node::Leaf(0)))
            } else if matches!(**b, Node::Leaf(id) if id == target) {
                Some(std::mem::replace(&mut **a, Node::Leaf(0)))
            } else {
                None
            };
            match sibling {
                Some(sibling) => {
                    *node = sibling;
                    true
                }
                None => go(a, target) || go(b, target),
            }
        }
        go(&mut self.root, target)
    }

    /// Panes in reading order (left-to-right, top-to-bottom within splits).
    pub fn panes(&self) -> Vec<PaneId> {
        fn go(node: &Node, out: &mut Vec<PaneId>) {
            match node {
                Node::Leaf(id) => out.push(*id),
                Node::Split { a, b, .. } => {
                    go(a, out);
                    go(b, out);
                }
            }
        }
        let mut out = Vec::new();
        go(&self.root, &mut out);
        out
    }

    pub fn contains(&self, pane: PaneId) -> bool {
        self.panes().contains(&pane)
    }

    pub fn len(&self) -> usize {
        self.panes().len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    /// Rectangles of every pane, with `gap` pixels between siblings.
    pub fn layout(&self, area: Rect, gap: f32) -> Vec<(PaneId, Rect)> {
        let mut out = Vec::new();
        self.walk(area, gap, &mut |node, rect, _| {
            if let Node::Leaf(id) = node {
                out.push((*id, rect));
            }
        });
        out
    }

    /// Draggable dividers between panes.
    pub fn dividers(&self, area: Rect, gap: f32) -> Vec<Divider> {
        let mut out = Vec::new();
        self.walk(area, gap, &mut |node, rect, path| {
            if let Node::Split { dir, ratio, .. } = node {
                let (ra, _) = split_rect(rect, *dir, *ratio, gap);
                let div = match dir {
                    SplitDir::Right => Rect::new(ra.right(), rect.y, gap, rect.h),
                    SplitDir::Down => Rect::new(rect.x, ra.bottom(), rect.w, gap),
                };
                out.push(Divider {
                    path: path.to_vec(),
                    dir: *dir,
                    rect: div,
                    area: rect,
                });
            }
        });
        out
    }

    /// Sets the ratio of the split at `path` from a pointer position inside `divider.area`.
    pub fn drag_divider(&mut self, divider: &Divider, px: f32, py: f32) {
        let ratio = match divider.dir {
            SplitDir::Right => (px - divider.area.x) / divider.area.w.max(1.0),
            SplitDir::Down => (py - divider.area.y) / divider.area.h.max(1.0),
        };
        let mut node = &mut self.root;
        for &second in &divider.path {
            match node {
                Node::Split { a, b, .. } => node = if second { b } else { a },
                Node::Leaf(_) => return,
            }
        }
        if let Node::Split { ratio: r, .. } = node {
            *r = ratio.clamp(MIN_RATIO, 1.0 - MIN_RATIO);
        }
    }

    /// The nearest pane in `dir` from `from`, by geometry.
    pub fn neighbor(&self, area: Rect, gap: f32, from: PaneId, dir: Direction) -> Option<PaneId> {
        let rects = self.layout(area, gap);
        let (_, f) = *rects.iter().find(|(id, _)| *id == from)?;
        let eps = gap + 1.0;
        let overlap = |a0: f32, a1: f32, b0: f32, b1: f32| (a1.min(b1) - a0.max(b0)).max(0.0);
        rects
            .iter()
            .filter(|(id, _)| *id != from)
            .filter_map(|(id, r)| {
                let (dist, ov) = match dir {
                    Direction::Right => {
                        (r.x - f.right(), overlap(f.y, f.bottom(), r.y, r.bottom()))
                    }
                    Direction::Left => (f.x - r.right(), overlap(f.y, f.bottom(), r.y, r.bottom())),
                    Direction::Down => (r.y - f.bottom(), overlap(f.x, f.right(), r.x, r.right())),
                    Direction::Up => (f.y - r.bottom(), overlap(f.x, f.right(), r.x, r.right())),
                };
                (dist >= -eps && ov > 0.0).then_some((*id, dist, ov))
            })
            // Closest first; among equals, the one sharing the longest edge.
            .min_by(|a, b| a.1.total_cmp(&b.1).then(b.2.total_cmp(&a.2)))
            .map(|(id, _, _)| id)
    }

    fn walk(&self, area: Rect, gap: f32, f: &mut impl FnMut(&Node, Rect, &[bool])) {
        fn go(
            node: &Node,
            rect: Rect,
            gap: f32,
            path: &mut Vec<bool>,
            f: &mut impl FnMut(&Node, Rect, &[bool]),
        ) {
            f(node, rect, path);
            if let Node::Split { dir, ratio, a, b } = node {
                let (ra, rb) = split_rect(rect, *dir, *ratio, gap);
                path.push(false);
                go(a, ra, gap, path, f);
                path.pop();
                path.push(true);
                go(b, rb, gap, path, f);
                path.pop();
            }
        }
        go(&self.root, area, gap, &mut Vec::new(), f);
    }
}

/// Splits `rect` into two children separated by `gap`, snapped to whole pixels.
fn split_rect(rect: Rect, dir: SplitDir, ratio: f32, gap: f32) -> (Rect, Rect) {
    match dir {
        SplitDir::Right => {
            let wa = ((rect.w - gap) * ratio).round().max(0.0);
            let wb = (rect.w - gap - wa).max(0.0);
            (
                Rect::new(rect.x, rect.y, wa, rect.h),
                Rect::new(rect.x + wa + gap, rect.y, wb, rect.h),
            )
        }
        SplitDir::Down => {
            let ha = ((rect.h - gap) * ratio).round().max(0.0);
            let hb = (rect.h - gap - ha).max(0.0);
            (
                Rect::new(rect.x, rect.y, rect.w, ha),
                Rect::new(rect.x, rect.y + ha + gap, rect.w, hb),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(0.0, 0.0, 1001.0, 501.0);

    #[test]
    fn split_and_layout() {
        let mut t = SplitTree::new(1);
        assert!(t.split(1, SplitDir::Right, 2));
        assert!(t.split(2, SplitDir::Down, 3));
        assert_eq!(t.panes(), vec![1, 2, 3]);
        let l = t.layout(AREA, 1.0);
        assert_eq!(l[0], (1, Rect::new(0.0, 0.0, 500.0, 501.0)));
        assert_eq!(l[1], (2, Rect::new(501.0, 0.0, 500.0, 250.0)));
        assert_eq!(l[2], (3, Rect::new(501.0, 251.0, 500.0, 250.0)));
    }

    #[test]
    fn remove_gives_space_to_sibling() {
        let mut t = SplitTree::new(1);
        t.split(1, SplitDir::Right, 2);
        t.split(2, SplitDir::Down, 3);
        assert!(t.remove(2));
        assert_eq!(t.panes(), vec![1, 3]);
        assert_eq!(
            t.layout(AREA, 1.0)[1],
            (3, Rect::new(501.0, 0.0, 500.0, 501.0))
        );
        assert!(t.remove(1));
        assert_eq!(t.panes(), vec![3]);
        // The last pane cannot be removed.
        assert!(!t.remove(3));
    }

    #[test]
    fn neighbors() {
        let mut t = SplitTree::new(1);
        t.split(1, SplitDir::Right, 2);
        t.split(2, SplitDir::Down, 3);
        assert_eq!(t.neighbor(AREA, 1.0, 1, Direction::Right), Some(2));
        assert_eq!(t.neighbor(AREA, 1.0, 3, Direction::Left), Some(1));
        assert_eq!(t.neighbor(AREA, 1.0, 2, Direction::Down), Some(3));
        assert_eq!(t.neighbor(AREA, 1.0, 3, Direction::Up), Some(2));
        assert_eq!(t.neighbor(AREA, 1.0, 1, Direction::Left), None);
    }

    #[test]
    fn dividers_and_drag() {
        let mut t = SplitTree::new(1);
        t.split(1, SplitDir::Right, 2);
        let d = t.dividers(AREA, 1.0);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].rect, Rect::new(500.0, 0.0, 1.0, 501.0));
        t.drag_divider(&d[0], 250.0, 10.0);
        assert_eq!(t.layout(AREA, 1.0)[0].1.w, 250.0);
        // Clamped so a pane never disappears.
        t.drag_divider(&d[0], -100.0, 10.0);
        assert!(t.layout(AREA, 1.0)[0].1.w > 0.0);
    }
}
