//! Faithful port of `viewer/src/routing/priorityQueue.js`.
//!
//! Translation notes:
//!
//! The JS `createMinHeap` is a standard binary min-heap keyed on `item.distance`
//! (f64). Comparison semantics:
//!
//! - Sift-up stops when `parent.distance <= child.distance` (strict `<` would
//!   always stop at equal; `<=` stops at equal too — so equal distances are left
//!   as-is, no swap).
//! - Sift-down picks the child whose `.distance < smallest.distance` (strict
//!   `<`). On ties the existing `smallest` wins, preserving the element that was
//!   at the shallower heap level — effectively FIFO within equal-distance items
//!   as they were when last reorganised.
//!
//! There is no secondary comparator; tie-breaking behaviour matches JS exactly
//! because we apply the same strict-less-than comparisons.
//!
//! The heap stores `T` items where `T: HasDistance`. Callers pass any struct
//! with a `distance: f64` field by implementing the trait.

/// Any item storable in the min-heap.
pub trait HasDistance {
    fn distance(&self) -> f64;
}

/// A binary min-heap keyed on `HasDistance::distance`, matching the JS
/// `createMinHeap` exactly.
pub struct MinHeap<T: HasDistance> {
    values: Vec<T>,
}

impl<T: HasDistance> Default for MinHeap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: HasDistance> MinHeap<T> {
    pub fn new() -> Self {
        MinHeap { values: Vec::new() }
    }

    pub fn size(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Push an item onto the heap.
    ///
    /// Sift-up: swap with parent while `parent.distance > child.distance`
    /// (i.e. stop when `parent.distance <= child.distance`, matching JS).
    pub fn push(&mut self, item: T) {
        self.values.push(item);
        let mut index = self.values.len() - 1;
        while index > 0 {
            let parent = (index - 1) / 2; // JS: Math.floor((index-1)/2)
            if self.values[parent].distance() <= self.values[index].distance() {
                break;
            }
            self.values.swap(parent, index);
            index = parent;
        }
    }

    /// Pop the minimum-distance item from the heap.
    ///
    /// Returns `None` when empty (JS returns `null`).
    pub fn pop(&mut self) -> Option<T> {
        if self.values.is_empty() {
            return None;
        }
        // Move root to return, put last element at root, sift down.
        let last = self.values.pop().unwrap();
        if self.values.is_empty() {
            // We just popped the only element; `last` *was* the root.
            return Some(last);
        }
        // Swap root out: take root (the minimum), place `last` there.
        let root = std::mem::replace(&mut self.values[0], last);

        // Sift down.
        let mut index = 0;
        loop {
            let left = index * 2 + 1;
            let right = left + 1;
            let mut smallest = index;
            if left < self.values.len()
                && self.values[left].distance() < self.values[smallest].distance()
            {
                smallest = left;
            }
            if right < self.values.len()
                && self.values[right].distance() < self.values[smallest].distance()
            {
                smallest = right;
            }
            if smallest == index {
                break;
            }
            self.values.swap(index, smallest);
            index = smallest;
        }

        Some(root)
    }
}

// ---------------------------------------------------------------------------
// A concrete item type for tests, matching the JS heap's expected shape.
// ---------------------------------------------------------------------------

/// A heap item with a `distance` field, matching the JS `{ distance }` shape.
#[derive(Debug, Clone, PartialEq)]
pub struct HeapItem {
    pub distance: f64,
}

impl HasDistance for HeapItem {
    fn distance(&self) -> f64 {
        self.distance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(d: f64) -> HeapItem {
        HeapItem { distance: d }
    }

    #[test]
    fn empty_heap_pop_returns_none() {
        let mut h: MinHeap<HeapItem> = MinHeap::new();
        assert_eq!(h.size(), 0);
        assert!(h.pop().is_none());
    }

    #[test]
    fn single_element() {
        let mut h = MinHeap::new();
        h.push(item(5.0));
        assert_eq!(h.size(), 1);
        assert_eq!(h.pop().unwrap().distance, 5.0);
        assert!(h.pop().is_none());
    }

    #[test]
    fn pops_in_ascending_distance_order() {
        let mut h = MinHeap::new();
        h.push(item(3.0));
        h.push(item(1.0));
        h.push(item(2.0));
        assert_eq!(h.pop().unwrap().distance, 1.0);
        assert_eq!(h.pop().unwrap().distance, 2.0);
        assert_eq!(h.pop().unwrap().distance, 3.0);
        assert!(h.pop().is_none());
    }

    #[test]
    fn reverse_insert_order_still_pops_ascending() {
        let mut h = MinHeap::new();
        for d in [10.0, 7.0, 4.0, 9.0, 1.0, 6.0] {
            h.push(item(d));
        }
        let mut out = Vec::new();
        while let Some(x) = h.pop() {
            out.push(x.distance);
        }
        assert_eq!(out, vec![1.0, 4.0, 6.0, 7.0, 9.0, 10.0]);
    }

    #[test]
    fn tie_distances_all_returned() {
        // When multiple items have the same distance, all must be returned
        // (heap doesn't lose items on ties).
        let mut h = MinHeap::new();
        h.push(item(2.0));
        h.push(item(2.0));
        h.push(item(2.0));
        let mut count = 0;
        while h.pop().is_some() {
            count += 1;
        }
        assert_eq!(count, 3);
    }

    #[test]
    fn size_tracks_correctly() {
        let mut h = MinHeap::new();
        assert_eq!(h.size(), 0);
        h.push(item(1.0));
        assert_eq!(h.size(), 1);
        h.push(item(2.0));
        assert_eq!(h.size(), 2);
        h.pop();
        assert_eq!(h.size(), 1);
        h.pop();
        assert_eq!(h.size(), 0);
    }

    #[test]
    fn interleaved_push_pop() {
        // Mirrors a Dijkstra-like access pattern: push, pop-min, push more.
        let mut h = MinHeap::new();
        h.push(item(5.0));
        h.push(item(3.0));
        assert_eq!(h.pop().unwrap().distance, 3.0); // pop 3
        h.push(item(1.0));
        h.push(item(4.0));
        assert_eq!(h.pop().unwrap().distance, 1.0); // pop 1
        assert_eq!(h.pop().unwrap().distance, 4.0); // pop 4
        assert_eq!(h.pop().unwrap().distance, 5.0); // pop 5
        assert!(h.pop().is_none());
    }

    #[test]
    fn negative_distances_ordered_correctly() {
        let mut h = MinHeap::new();
        h.push(item(-1.0));
        h.push(item(0.0));
        h.push(item(-5.0));
        assert_eq!(h.pop().unwrap().distance, -5.0);
        assert_eq!(h.pop().unwrap().distance, -1.0);
        assert_eq!(h.pop().unwrap().distance, 0.0);
    }

    #[test]
    fn sift_up_stops_at_equal_parent() {
        // Parent distance == child distance → no swap (JS: `<=` stops).
        // Both elements should still be retrievable in some order.
        let mut h = MinHeap::new();
        h.push(item(4.0));
        h.push(item(4.0));
        assert_eq!(h.size(), 2);
        assert_eq!(h.pop().unwrap().distance, 4.0);
        assert_eq!(h.pop().unwrap().distance, 4.0);
    }
}
