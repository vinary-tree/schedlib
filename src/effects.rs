use core::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::ResourceId;

/// Canonical sorted set of effect resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectSet {
    resources: Vec<ResourceId>,
}

impl EffectSet {
    /// Constructs a sorted, duplicate-free effect set without recursive sort.
    #[must_use]
    pub fn from_resources(resources: impl IntoIterator<Item = ResourceId>) -> Self {
        let iterator = resources.into_iter();
        let mut heap = BinaryHeap::with_capacity(iterator.size_hint().0);
        heap.extend(iterator.map(Reverse));
        let mut canonical = Vec::with_capacity(heap.len());
        while let Some(Reverse(resource)) = heap.pop() {
            if canonical.last().copied() != Some(resource) {
                canonical.push(resource);
            }
        }
        Self {
            resources: canonical,
        }
    }

    /// Returns canonical resources in strictly increasing order.
    pub fn iter(&self) -> core::slice::Iter<'_, ResourceId> {
        self.resources.iter()
    }

    /// Returns canonical resources as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[ResourceId] {
        &self.resources
    }

    /// Returns whether this set contains no resources.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }

    /// Returns the number of resources.
    #[must_use]
    pub fn len(&self) -> usize {
        self.resources.len()
    }
}

impl<'a> IntoIterator for &'a EffectSet {
    type Item = &'a ResourceId;
    type IntoIter = core::slice::Iter<'a, ResourceId>;

    fn into_iter(self) -> Self::IntoIter {
        self.resources.iter()
    }
}

/// Canonical read and write effects for one task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskEffects {
    reads: EffectSet,
    writes: EffectSet,
}

impl TaskEffects {
    /// Creates effect metadata from canonicalizing read and write sets.
    #[must_use]
    pub const fn new(reads: EffectSet, writes: EffectSet) -> Self {
        Self { reads, writes }
    }

    /// Returns the read resources.
    #[must_use]
    pub fn reads(&self) -> &[ResourceId] {
        self.reads.as_slice()
    }

    /// Returns the write resources.
    #[must_use]
    pub fn writes(&self) -> &[ResourceId] {
        self.writes.as_slice()
    }

    /// Returns whether two tasks have no read/write or write/write hazard.
    #[must_use]
    pub fn is_independent_with(&self, other: &Self) -> bool {
        self.prove_independent(other).is_some()
    }

    /// Returns an unforgeable witness exactly when the symmetric effect kernel
    /// proves independence.
    #[must_use]
    pub fn prove_independent(&self, other: &Self) -> Option<IndependenceWitness> {
        let left_writes_safe =
            disjoint(self.writes(), other.reads()) && disjoint(self.writes(), other.writes());
        let right_writes_safe =
            disjoint(other.writes(), self.reads()) && disjoint(other.writes(), self.writes());
        (left_writes_safe && right_writes_safe).then_some(IndependenceWitness { private: () })
    }
}

/// Proof token showing that the shared symmetric effect kernel accepted a
/// pair of task-effect descriptions.
///
/// Callers cannot construct this token directly; it is returned only by
/// [`TaskEffects::prove_independent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndependenceWitness {
    private: (),
}

fn disjoint(left: &[ResourceId], right: &[ResourceId]) -> bool {
    let mut left_index = 0usize;
    let mut right_index = 0usize;
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            core::cmp::Ordering::Less => left_index += 1,
            core::cmp::Ordering::Equal => return false,
            core::cmp::Ordering::Greater => right_index += 1,
        }
    }
    true
}
