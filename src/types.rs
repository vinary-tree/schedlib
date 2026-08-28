use core::fmt;
use core::num::NonZeroU64;

/// Stable caller-visible task identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct TaskId(u32);

impl TaskId {
    /// Creates a task identifier from its stable representation.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the stable representation.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Returns the identifier as an indexing value.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Stable caller-visible effect resource identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ResourceId(u32);

impl ResourceId {
    /// Creates a resource identifier from its stable representation.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the stable representation.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Strictly positive resource cost for one task or batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Cost(NonZeroU64);

impl Cost {
    /// Creates a positive cost, returning `None` for zero.
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the positive cost.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Strictly positive per-batch resource budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Budget(NonZeroU64);

impl Budget {
    /// Creates a positive budget, returning `None` for zero.
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the positive budget.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}
