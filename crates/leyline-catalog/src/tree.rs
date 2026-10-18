//! Assembles the collection and keyword trees from their parent links.
//!
//! The rows come from a file anything can edit (ADR 0152 §1): a parent may
//! be missing, a row may be its own parent, two rows may point at each
//! other. None of it stops the read, and every row appears exactly once.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// A node that owns its children, kept sorted by name.
pub(crate) trait TreeNode: Sized {
    fn name(&self) -> &str;
    fn children_mut(&mut self) -> &mut Vec<Self>;
}

/// Moves each node under its parent and returns the roots, sorted by name.
///
/// `parents` maps a node id to its parent id. A node whose parent is absent
/// or itself is a root; a cycle is cut at its smallest id, which becomes a
/// root. Ids say nothing about depth — a collection moves under a newer one
/// — so the order comes from a walk down from the roots, not from the ids.
pub(crate) fn assemble<N: TreeNode>(
    mut nodes: BTreeMap<i64, N>,
    parents: &BTreeMap<i64, i64>,
) -> Vec<N> {
    let mut children: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    for (&id, &parent) in parents {
        if parent != id && nodes.contains_key(&parent) {
            children.entry(parent).or_default().push(id);
        }
    }
    let is_root = |id: &i64| {
        parents
            .get(id)
            .is_none_or(|parent| parent == id || !nodes.contains_key(parent))
    };

    // Breadth-first from the roots, then from whatever a cycle kept out of
    // reach: every edge `(child, parent)` is listed after its parent's own.
    let mut seen = BTreeSet::new();
    let mut edges = Vec::new();
    let starts: Vec<i64> = nodes
        .keys()
        .copied()
        .filter(is_root)
        .chain(nodes.keys().copied())
        .collect();
    for start in starts {
        if !seen.insert(start) {
            continue;
        }
        let mut queue = VecDeque::from([start]);
        while let Some(id) = queue.pop_front() {
            for &child in children.get(&id).into_iter().flatten() {
                if seen.insert(child) {
                    edges.push((child, id));
                    queue.push_back(child);
                }
            }
        }
    }

    // Deepest first, so a node is complete when it moves under its parent.
    for (id, parent) in edges.into_iter().rev() {
        let Some(node) = nodes.remove(&id) else {
            continue;
        };
        match nodes.get_mut(&parent) {
            Some(parent) => {
                let siblings = parent.children_mut();
                let at = siblings
                    .binary_search_by(|c| c.name().cmp(node.name()))
                    .unwrap_or_else(|e| e);
                siblings.insert(at, node);
            }
            // The walk lists a parent before its children, so it is still
            // here; were it not, the node stays a root rather than vanish.
            None => {
                nodes.insert(id, node);
            }
        }
    }

    let mut roots: Vec<N> = nodes.into_values().collect();
    roots.sort_by(|a, b| a.name().cmp(b.name()));
    roots
}
