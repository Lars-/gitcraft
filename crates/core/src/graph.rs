//! Lane layout for the commit graph.
//!
//! Commits are laid out one per row, children above parents. Each row gets the lane (column) of
//! its commit and the line segments to draw in it. When a line ends, the lines right of it close
//! up, so the graph is only as wide as the lines that run side by side; every line keeps its
//! colour. Only the rows on screen are ever built: the layout keeps a snapshot of the lanes every
//! 256 rows and replays from the nearest one, so even a history of millions of commits costs little
//! memory beyond the parent lists.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::ops::Range;

const CHECKPOINT: usize = 256;

/// How many colours the lines take turns in.
pub const COLOURS: u8 = 10;

/// The parents of every commit, as indices into the same list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parents {
    start: Vec<u32>,
    list: Vec<u32>,
}

impl Parents {
    /// Appends the next commit with the given parents.
    pub fn push(&mut self, parents: impl IntoIterator<Item = usize>) {
        if self.start.is_empty() {
            self.start.push(0);
        }
        self.list.extend(parents.into_iter().map(index));
        self.start.push(index(self.list.len()));
    }

    /// The parents of commit `i`.
    pub fn of(&self, i: usize) -> impl ExactSizeIterator<Item = usize> + '_ {
        self.list[self.start[i] as usize..self.start[i + 1] as usize].iter().map(|&p| p as usize)
    }

    /// The number of commits.
    pub fn len(&self) -> usize {
        self.start.len().saturating_sub(1)
    }

    /// Whether there are no commits.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The same graph with the commits in `order`, which must be a permutation of all indices.
    #[must_use]
    pub fn reorder(&self, order: &[usize]) -> Self {
        let mut position = vec![0; order.len()];
        for (new, &old) in order.iter().enumerate() {
            position[old] = new;
        }
        let mut reordered = Self::default();
        for &old in order {
            reordered.push(self.of(old).map(|p| position[p]));
        }
        reordered
    }
}

fn index(i: usize) -> u32 {
    u32::try_from(i).expect("fewer than 4 billion commits")
}

/// The order `git log --date-order` uses: every commit before its parents, newest first otherwise.
pub fn date_order(times: &[i64], parents: &Parents) -> Vec<usize> {
    let mut children = vec![0_u32; times.len()];
    for i in 0..times.len() {
        for p in parents.of(i) {
            children[p] += 1;
        }
    }
    let mut ready: BinaryHeap<_> =
        (0..times.len()).filter(|&i| children[i] == 0).map(|i| (times[i], Reverse(i))).collect();
    let mut order = Vec::with_capacity(times.len());
    while let Some((_, Reverse(i))) = ready.pop() {
        order.push(i);
        for p in parents.of(i) {
            children[p] -= 1;
            if children[p] == 0 {
                ready.push((times[p], Reverse(p)));
            }
        }
    }
    order
}

/// Which half of a row a line segment is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Half {
    /// From the top edge of the row to its middle, where the commit sits.
    Top,
    /// From the middle of the row to its bottom edge.
    Bottom,
}

/// A line segment in one row, between two lanes. Lanes that only pass through have `from == to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    /// The lane at the top of the segment.
    pub from: usize,
    /// The lane at the bottom of the segment.
    pub to: usize,
    /// The half of the row the segment is in.
    pub half: Half,
    /// The colour of the line the segment is part of, below [`COLOURS`].
    pub colour: u8,
}

/// One row of the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The lane of the row's commit.
    pub lane: usize,
    /// The colour of the row's commit, below [`COLOURS`].
    pub colour: u8,
    /// The number of lanes in use in this row.
    pub width: usize,
    /// The segments to draw.
    pub edges: Vec<Edge>,
}

/// A line in a lane: the commit it is heading for, and its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Line {
    to: usize,
    colour: u8,
}

/// The line in every lane (a lane is empty only while a row is laid out), and the colour the next
/// new line takes.
#[derive(Debug, Clone, Default)]
struct Lanes {
    lines: Vec<Option<Line>>,
    next: u8,
}

impl Lanes {
    fn free(&mut self) -> usize {
        self.lines.iter().position(Option::is_none).unwrap_or_else(|| {
            self.lines.push(None);
            self.lines.len() - 1
        })
    }

    fn heading(&self, commit: usize) -> Option<usize> {
        self.lines.iter().position(|l| l.is_some_and(|l| l.to == commit))
    }

    /// The colour of a new line in `lane`: new lines take turns in the colours, skipping the colours
    /// of the lines beside it (of three in a row, one is not beside it).
    fn colour(&mut self, lane: usize) -> u8 {
        let beside = [lane.wrapping_sub(1), lane + 1].map(|i| self.lines.get(i).copied().flatten().map(|l| l.colour));
        let mut turns = (self.next..self.next + 3).map(|c| c % COLOURS);
        let colour = turns.find(|c| !beside.contains(&Some(*c))).unwrap_or(self.next);
        self.next = (colour + 1) % COLOURS;
        colour
    }

    /// Lays out the row of `commit`. Lines that head for the same commit share a lane, the one
    /// furthest left, so no two lines ever meet at a commit: they join below the child instead.
    fn step(&mut self, commit: usize, parents: impl Iterator<Item = usize>) -> Row {
        let lane = self.heading(commit).unwrap_or_else(|| self.free());
        let above =
            |(i, l): (usize, &Option<Line>)| l.map(|l| Edge { from: i, to: i, half: Half::Top, colour: l.colour });
        let mut edges = Vec::with_capacity(2 * self.lines.len() + 2);
        edges.extend(self.lines.iter().enumerate().filter_map(above));
        let colour = match self.lines[lane].take() {
            Some(l) => l.colour,
            None => self.colour(lane),
        };
        let mut fed = Vec::new();
        for (n, parent) in parents.enumerate() {
            let existing = self.heading(parent);
            let to = match (n, existing) {
                (0, Some(existing)) if existing < lane => existing,
                (0, _) => lane,
                (_, Some(existing)) => existing,
                (_, None) => self.free(),
            };
            if n == 0
                && let Some(existing) = existing.filter(|&e| e > lane)
            {
                self.lines[existing] = None;
                edges.push(Edge { from: existing, to: lane, half: Half::Bottom, colour });
            }
            let colour = match self.lines[to] {
                Some(l) => l.colour,
                None if n == 0 => colour,
                None => self.colour(to),
            };
            if self.lines[to].is_none() {
                fed.push(to);
            }
            self.lines[to] = Some(Line { to: parent, colour });
            edges.push(Edge { from: lane, to, half: Half::Bottom, colour });
        }
        for (i, l) in self.lines.iter().enumerate() {
            if let Some(line) = l
                && !fed.contains(&i)
            {
                edges.push(Edge { from: i, to: i, half: Half::Bottom, colour: line.colour });
            }
        }
        let width = self.lines.len();
        if self.lines.iter().any(Option::is_none) {
            // Every line moves left past the empty lanes before it.
            let empty_before: Vec<usize> = self
                .lines
                .iter()
                .scan(0, |empty, l| {
                    let before = *empty;
                    *empty += usize::from(l.is_none());
                    Some(before)
                })
                .collect();
            for edge in edges.iter_mut().filter(|e| e.half == Half::Bottom) {
                edge.to -= empty_before[edge.to];
            }
            self.lines.retain(Option::is_some);
        }
        Row { lane, colour, width, edges }
    }
}

/// The laid-out graph of a whole history.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    parents: Parents,
    checkpoints: Vec<Lanes>,
    width: usize,
}

impl Graph {
    /// Lays out commits whose parents always come after them.
    pub fn new(parents: Parents) -> Self {
        let mut lanes = Lanes::default();
        let mut checkpoints = Vec::new();
        let mut width = 0;
        for i in 0..parents.len() {
            if i % CHECKPOINT == 0 {
                checkpoints.push(lanes.clone());
            }
            width = width.max(lanes.step(i, parents.of(i)).width);
        }
        Self { parents, checkpoints, width }
    }

    /// The number of rows.
    pub fn len(&self) -> usize {
        self.parents.len()
    }

    /// Whether the graph has no rows.
    pub fn is_empty(&self) -> bool {
        self.parents.is_empty()
    }

    /// The most lanes any row uses.
    pub fn width(&self) -> usize {
        self.width
    }

    /// The parents of the commit in row `i`.
    pub fn parents(&self, i: usize) -> impl ExactSizeIterator<Item = usize> + '_ {
        self.parents.of(i)
    }

    /// The rows in `range`, which is clamped to the graph.
    pub fn rows(&self, range: Range<usize>) -> Vec<Row> {
        let end = range.end.min(self.len());
        let first = range.start.min(end) / CHECKPOINT * CHECKPOINT;
        let Some(lanes) = self.checkpoints.get(first / CHECKPOINT) else {
            return Vec::new();
        };
        let mut lanes = lanes.clone();
        (first..end).map(|i| lanes.step(i, self.parents.of(i))).skip(range.start.saturating_sub(first)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn parents(lists: &[&[usize]]) -> Parents {
        let mut p = Parents::default();
        for list in lists {
            p.push(list.iter().copied());
        }
        p
    }

    fn top(from: usize, to: usize, colour: u8) -> Edge {
        Edge { from, to, half: Half::Top, colour }
    }

    fn bottom(from: usize, to: usize, colour: u8) -> Edge {
        Edge { from, to, half: Half::Bottom, colour }
    }

    #[test]
    fn linear_history_stays_in_one_lane() {
        let graph = Graph::new(parents(&[&[1], &[2], &[]]));
        let rows = graph.rows(0..3);
        assert_eq!(rows[0].edges, [bottom(0, 0, 0)]);
        assert_eq!(rows[1].edges, [top(0, 0, 0), bottom(0, 0, 0)]);
        assert_eq!(rows[2].edges, [top(0, 0, 0)]);
        assert!(rows.iter().all(|r| r.lane == 0));
        assert_eq!(graph.width(), 1);
    }

    #[test]
    fn a_merge_opens_a_lane_that_joins_the_line_to_the_fork() {
        // 0 merges 1 (main) and 2 (feature); both come from 3.
        let graph = Graph::new(parents(&[&[1, 2], &[3], &[3], &[]]));
        let rows = graph.rows(0..4);
        assert_eq!(rows[0].edges, [bottom(0, 0, 0), bottom(0, 1, 1)]);
        assert_eq!((rows[1].lane, rows[2].lane, rows[3].lane), (0, 1, 0));
        assert_eq!(rows[2].edges, [top(0, 0, 0), top(1, 1, 1), bottom(1, 0, 0), bottom(0, 0, 0)]);
        assert_eq!(rows[3].edges, [top(0, 0, 0)]);
        assert_eq!(graph.width(), 2);
    }

    #[test]
    fn a_second_tip_takes_a_free_lane_and_joins_the_line_to_its_parent() {
        let graph = Graph::new(parents(&[&[2], &[2], &[]]));
        let rows = graph.rows(0..3);
        assert_eq!((rows[1].lane, rows[1].colour), (1, 1));
        assert_eq!(rows[1].edges, [top(0, 0, 0), bottom(1, 0, 0), bottom(0, 0, 0)]);
        assert_eq!(rows[2].edges, [top(0, 0, 0)]);
    }

    #[test]
    fn a_line_right_of_a_commit_joins_it_when_both_head_for_its_parent() {
        // 0 heads for 2 in lane 0; 1 sits in lane 1 and merges 2; 2 and the line of 1 both head for 3.
        let graph = Graph::new(parents(&[&[2], &[3, 2], &[3], &[]]));
        let rows = graph.rows(0..4);
        assert_eq!(rows[1].edges, [top(0, 0, 0), bottom(1, 1, 1), bottom(1, 0, 0), bottom(0, 0, 0)]);
        assert_eq!(rows[2].edges, [top(0, 0, 0), top(1, 1, 1), bottom(1, 0, 0), bottom(0, 0, 0)]);
        assert_eq!(rows[3].edges, [top(0, 0, 0)]);
    }

    #[test]
    fn the_lines_right_of_a_line_that_ends_close_up_in_their_colours() {
        // 3 joins the line of 0 to 4, so the line of 2 to 5 moves into its lane.
        let graph = Graph::new(parents(&[&[4], &[3], &[5], &[4], &[5], &[]]));
        let rows = graph.rows(0..6);
        let row = [top(0, 0, 0), top(1, 1, 1), top(2, 2, 2), bottom(1, 0, 0), bottom(0, 0, 0), bottom(2, 1, 2)];
        assert_eq!(rows[3].edges, row);
        assert_eq!(rows[4].edges, [top(0, 0, 0), top(1, 1, 2), bottom(1, 0, 0), bottom(0, 0, 0)]);
        assert_eq!((rows[3].width, rows[4].width, graph.width()), (3, 2, 3));
    }

    #[test]
    fn new_lines_take_turns_in_the_colours_but_not_in_that_of_a_line_beside_them() {
        // The line of 0 to 11 runs down lane 0 in colour 0; 1 to 10 each start a line beside it.
        let mut lists = vec![&[11][..]; 11];
        lists.push(&[]);
        let graph = Graph::new(parents(&lists));
        let colours: Vec<_> = graph.rows(0..11).iter().map(|row| (row.lane, row.colour)).collect();
        let expected: Vec<_> = (0..10).map(|c| (usize::from(c > 0), c)).chain([(1, 1)]).collect();
        assert_eq!(colours, expected);
    }

    #[test]
    fn rows_are_clamped_to_the_graph() {
        let graph = Graph::new(parents(&[&[1], &[]]));
        assert_eq!(graph.rows(1..10).len(), 1);
        assert_eq!(graph.rows(5..10), []);
        assert_eq!(Graph::default().rows(0..1), []);
    }

    #[test]
    fn date_order_puts_children_first_even_with_skewed_clocks() {
        // 1 is the parent of 0 but has a later time.
        let order = date_order(&[10, 20, 15], &parents(&[&[1], &[], &[]]));
        assert_eq!(order, [2, 0, 1]);
    }

    /// Random histories: commit `i` only has parents after it, at most three of them.
    fn histories() -> impl Strategy<Value = Parents> {
        (1..600_usize)
            .prop_flat_map(|n| {
                (0..n).map(|i| proptest::collection::vec(i + 1..n.max(i + 2), 0..=3)).collect::<Vec<_>>()
            })
            .prop_map(|lists| {
                let n = lists.len();
                let mut p = Parents::default();
                for list in lists {
                    p.push(list.into_iter().filter(|&q| q < n));
                }
                p
            })
    }

    proptest! {
        #[test]
        fn lines_never_dangle(p in histories()) {
            let graph = Graph::new(p.clone());
            let rows = graph.rows(0..graph.len());
            for (i, pair) in rows.windows(2).enumerate() {
                let mut down: Vec<_> = pair[0].edges.iter().filter(|e| e.half == Half::Bottom).map(|e| e.to).collect();
                let mut up: Vec<_> = pair[1].edges.iter().filter(|e| e.half == Half::Top).map(|e| e.from).collect();
                down.sort_unstable();
                down.dedup();
                up.sort_unstable();
                up.dedup();
                prop_assert_eq!(&down, &up, "between rows {} and {}", i, i + 1);
                prop_assert_eq!(down, (0..up.len()).collect::<Vec<_>>(), "no empty lane below row {}", i);
            }
            for (i, row) in rows.iter().enumerate() {
                let from_commit = row.edges.iter().filter(|e| e.half == Half::Bottom && e.from == row.lane).count();
                prop_assert_eq!(from_commit, p.of(i).len(), "row {}", i);
                prop_assert!(row.lane < row.width && row.width <= graph.width());
                prop_assert!(row.edges.iter().all(|e| e.from < row.width && e.to < row.width));
                prop_assert!(row.colour < COLOURS && row.edges.iter().all(|e| e.colour < COLOURS));
            }
        }

        #[test]
        fn lines_keep_their_colour(p in histories()) {
            let graph = Graph::new(p);
            let rows = graph.rows(0..graph.len());
            for (i, pair) in rows.windows(2).enumerate() {
                let mut down: Vec<_> =
                    pair[0].edges.iter().filter(|e| e.half == Half::Bottom).map(|e| (e.to, e.colour)).collect();
                let mut up: Vec<_> =
                    pair[1].edges.iter().filter(|e| e.half == Half::Top).map(|e| (e.from, e.colour)).collect();
                down.sort_unstable();
                down.dedup();
                up.sort_unstable();
                up.dedup();
                prop_assert_eq!(down, up, "between rows {} and {}", i, i + 1);
            }
            for row in &rows {
                let into = row.edges.iter().filter(|e| e.half == Half::Top && e.to == row.lane);
                prop_assert!(into.clone().all(|e| e.colour == row.colour), "{:?}", row);
            }
        }

        #[test]
        fn no_two_lines_head_for_the_same_commit(p in histories()) {
            let graph = Graph::new(p);
            for (i, row) in graph.rows(0..graph.len()).iter().enumerate() {
                let tops = row.edges.iter().filter(|e| e.half == Half::Top);
                prop_assert!(tops.clone().all(|e| e.from == e.to), "row {}: {:?}", i, row);
            }
        }

        #[test]
        fn any_window_matches_the_full_layout(p in histories(), start in 0..600_usize, len in 1..300_usize) {
            let graph = Graph::new(p);
            let all = graph.rows(0..graph.len());
            let start = start.min(graph.len());
            let end = (start + len).min(graph.len());
            prop_assert_eq!(graph.rows(start..end), all[start..end].to_vec());
        }

        #[test]
        fn date_order_is_topological(p in histories(), times in proptest::collection::vec(0..100_i64, 600)) {
            let order = date_order(&times[..p.len()], &p);
            prop_assert_eq!(order.len(), p.len());
            let reordered = p.reorder(&order);
            for i in 0..reordered.len() {
                prop_assert!(reordered.of(i).all(|q| q > i));
            }
        }
    }
}
