//! Stack-safe committed-prefix recovery for deterministic schedules.
//!
//! This module contains only semantic protocol state. Portable codecs,
//! cryptographic digests, filesystems, and runtime persistence policy belong
//! in downstream interoperability and runtime crates.

use core::cmp::Reverse;
use core::fmt;
use std::collections::BinaryHeap;
use std::sync::Arc;

use crate::TaskId;

/// An error detected before or during durable protocol execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableError {
    /// A caller-owned task key occurs more than once.
    DuplicateExternalKey,
    /// The task population cannot be represented by the dense identifier.
    TooManyTasks,
    /// Per-task effect or cost metadata does not match the key population.
    TaskMetadataLengthMismatch,
    /// A dependency references a key absent from the task population.
    UnknownDependencyKey,
    /// An event does not extend the canonical journal language.
    InvalidEvent,
    /// An append was attempted after a terminal event.
    TerminalJournal,
    /// A checkpoint belongs to a structurally different plan.
    ForeignPlan,
    /// A checkpoint is corrupt or internally inconsistent.
    MalformedCheckpoint,
    /// Publication was attempted before journal append or out of order.
    PublicationOutOfOrder,
    /// A codec accounting limit would be exceeded.
    CodecLimitExceeded,
    /// Recovery would repeat an effect without an explicit replay witness.
    UnsafeReplay,
    /// The supplied task outcomes do not cover the active plan exactly.
    OutcomeCountMismatch,
    /// Physical completion order is not a permutation of the task population.
    InvalidCompletionOrder,
    /// A replay key or crash ordinal is outside the active plan domain.
    InvalidProtocolConfiguration,
    /// The verified finite transition bound was exceeded.
    StepBoundExceeded,
}

impl fmt::Display for DurableError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::DuplicateExternalKey => "external task keys must be unique",
            Self::TooManyTasks => "task population exceeds the dense identifier domain",
            Self::TaskMetadataLengthMismatch => "task metadata lengths do not match",
            Self::UnknownDependencyKey => "dependency references an unknown task key",
            Self::InvalidEvent => "event is not a canonical journal extension",
            Self::TerminalJournal => "a terminal journal cannot grow",
            Self::ForeignPlan => "checkpoint plan identity does not match the active plan",
            Self::MalformedCheckpoint => "checkpoint is corrupt or internally inconsistent",
            Self::PublicationOutOfOrder => "publication requires the next durable event",
            Self::CodecLimitExceeded => "codec accounting limit would be exceeded",
            Self::UnsafeReplay => "pre-journal replay lacks an explicit safety witness",
            Self::OutcomeCountMismatch => "outcomes do not cover the active plan",
            Self::InvalidCompletionOrder => "completion order is not an exact permutation",
            Self::InvalidProtocolConfiguration => {
                "replay or crash configuration is outside the active plan domain"
            }
            Self::StepBoundExceeded => "durable recovery exceeded its verified step bound",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for DurableError {}

/// A bijection from canonical caller-owned keys to dense internal identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalKeyMap<K> {
    keys: Arc<Vec<K>>,
}

impl<K: Ord> ExternalKeyMap<K> {
    /// Builds a canonical, duplicate-free map.
    ///
    /// # Errors
    ///
    /// Returns [`DurableError::DuplicateExternalKey`] for duplicate keys and
    /// [`DurableError::TooManyTasks`] when dense identifiers cannot represent
    /// the population.
    pub fn new(keys: impl IntoIterator<Item = K>) -> Result<Self, DurableError> {
        let iterator = keys.into_iter();
        let mut supplied: Vec<K> = iterator.collect();
        if supplied.len() > u32::MAX as usize {
            return Err(DurableError::TooManyTasks);
        }
        if supplied.windows(2).all(|pair| pair[0] < pair[1]) {
            return Ok(Self {
                keys: Arc::new(supplied),
            });
        }

        let mut heap = BinaryHeap::with_capacity(supplied.len());
        heap.extend(supplied.drain(..).map(Reverse));
        while let Some(Reverse(key)) = heap.pop() {
            if supplied.last() == Some(&key) {
                return Err(DurableError::DuplicateExternalKey);
            }
            supplied.push(key);
        }
        Ok(Self {
            keys: Arc::new(supplied),
        })
    }

    fn from_canonical(keys: Vec<K>) -> Self {
        Self {
            keys: Arc::new(keys),
        }
    }

    /// Returns the caller key for a dense identifier.
    #[must_use]
    pub fn key(&self, dense: TaskId) -> Option<&K> {
        self.keys.get(dense.index())
    }

    /// Returns the dense identifier assigned to a caller key.
    #[must_use]
    pub fn dense(&self, key: &K) -> Option<TaskId> {
        self.keys
            .binary_search(key)
            .ok()
            .and_then(|index| u32::try_from(index).ok())
            .map(TaskId::new)
    }

    /// Iterates in increasing dense-identifier order.
    pub fn iter(&self) -> impl Iterator<Item = (&K, TaskId)> + '_ {
        self.keys
            .iter()
            .zip(0u32..)
            .map(|(key, dense)| (key, TaskId::new(dense)))
    }

    /// Returns the number of mapped task keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Returns whether the map contains no task keys.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TaskIdentityRow<K> {
    key: K,
    reads: Vec<u64>,
    writes: Vec<u64>,
    cost: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PlanSignatureData<K> {
    schema: u64,
    keys: Vec<K>,
    dependencies: Vec<(TaskId, TaskId)>,
    effects: Vec<(Vec<u64>, Vec<u64>)>,
    costs: Vec<u64>,
    budget: u64,
    semantic_profile: Arc<str>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PlanSignature<K>(Arc<PlanSignatureData<K>>);

/// Exact structural identity for a deterministic scheduling plan.
///
/// Equality binds the schema, canonical keys, dependencies, effects, costs,
/// budget, and semantic profile. No finite digest participates in equality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanIdentity<K, P> {
    signature: PlanSignature<K>,
    semantic_profile: P,
}

/// Profile-type-erased view of an exact structural plan identity.
///
/// The semantic profile's complete string value remains in the structure;
/// only its caller-side Rust representation is erased at the durable boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralPlanIdentity<K> {
    signature: PlanSignature<K>,
}

impl<K, P> PartialEq<PlanIdentity<K, P>> for StructuralPlanIdentity<K>
where
    K: PartialEq,
{
    fn eq(&self, other: &PlanIdentity<K, P>) -> bool {
        self.signature == other.signature
    }
}

impl<K, P> PartialEq<StructuralPlanIdentity<K>> for PlanIdentity<K, P>
where
    K: PartialEq,
{
    fn eq(&self, other: &StructuralPlanIdentity<K>) -> bool {
        self.signature == other.signature
    }
}

impl<K> StructuralPlanIdentity<K> {
    fn task_count(&self) -> usize {
        self.signature.0.keys.len()
    }
}

impl<K, P> PlanIdentity<K, P>
where
    K: Clone + Ord,
    P: AsRef<str> + Clone + Eq,
{
    /// Number of independently identity-bearing structural fields.
    pub const FIELD_COUNT: usize = 7;

    /// Canonicalizes and validates every structural identity field.
    ///
    /// # Errors
    ///
    /// Returns an error when task metadata lengths differ, a caller key is
    /// duplicated, a dependency key is unknown, or the dense domain is too
    /// small.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema: u64,
        keys: Vec<K>,
        dependencies: Vec<(K, K)>,
        effects: Vec<(Vec<u64>, Vec<u64>)>,
        costs: Vec<u64>,
        budget: u64,
        semantic_profile: P,
    ) -> Result<Self, DurableError> {
        if keys.len() != effects.len() || keys.len() != costs.len() {
            return Err(DurableError::TaskMetadataLengthMismatch);
        }
        if keys.len() > u32::MAX as usize {
            return Err(DurableError::TooManyTasks);
        }

        let keys_are_canonical = keys.windows(2).all(|pair| pair[0] < pair[1]);
        let mut supplied_rows = Vec::with_capacity(keys.len());
        for ((key, (reads, writes)), cost) in keys.into_iter().zip(effects).zip(costs) {
            supplied_rows.push(TaskIdentityRow {
                key,
                reads: canonical_u64_set(reads),
                writes: canonical_u64_set(writes),
                cost,
            });
        }

        let mut rows = if keys_are_canonical {
            supplied_rows
        } else {
            let mut heap = BinaryHeap::with_capacity(supplied_rows.len());
            heap.extend(supplied_rows.drain(..).map(Reverse));
            while let Some(Reverse(row)) = heap.pop() {
                supplied_rows.push(row);
            }
            supplied_rows
        };
        let mut canonical_keys = Vec::with_capacity(rows.len());
        let mut canonical_effects = Vec::with_capacity(rows.len());
        let mut canonical_costs = Vec::with_capacity(rows.len());
        for row in rows.drain(..) {
            if canonical_keys.last() == Some(&row.key) {
                return Err(DurableError::DuplicateExternalKey);
            }
            canonical_keys.push(row.key);
            canonical_effects.push((row.reads, row.writes));
            canonical_costs.push(row.cost);
        }

        let key_map = ExternalKeyMap::from_canonical(canonical_keys.clone());
        let mut supplied_dependencies = Vec::with_capacity(dependencies.len());
        for (source, target) in dependencies {
            let source = key_map
                .dense(&source)
                .ok_or(DurableError::UnknownDependencyKey)?;
            let target = key_map
                .dense(&target)
                .ok_or(DurableError::UnknownDependencyKey)?;
            supplied_dependencies.push((source, target));
        }
        let mut canonical_dependencies = if supplied_dependencies
            .windows(2)
            .all(|pair| pair[0] <= pair[1])
        {
            supplied_dependencies
        } else {
            let mut heap = BinaryHeap::with_capacity(supplied_dependencies.len());
            heap.extend(supplied_dependencies.drain(..).map(Reverse));
            while let Some(Reverse(edge)) = heap.pop() {
                supplied_dependencies.push(edge);
            }
            supplied_dependencies
        };
        canonical_dependencies.dedup();

        let signature = PlanSignature(Arc::new(PlanSignatureData {
            schema,
            keys: canonical_keys,
            dependencies: canonical_dependencies,
            effects: canonical_effects,
            costs: canonical_costs,
            budget,
            semantic_profile: Arc::from(semantic_profile.as_ref()),
        }));
        Ok(Self {
            signature,
            semantic_profile,
        })
    }

    /// Returns the caller-visible resource budget bound into this identity.
    #[must_use]
    pub fn budget(&self) -> u64 {
        self.signature.0.budget
    }

    /// Returns a structurally identical plan except for its budget.
    #[must_use]
    pub fn with_budget(&self, budget: u64) -> Self {
        let mut data = (*self.signature.0).clone();
        data.budget = budget;
        Self {
            signature: PlanSignature(Arc::new(data)),
            semantic_profile: self.semantic_profile.clone(),
        }
    }

    /// Returns a structurally identical plan except for its semantic profile.
    #[must_use]
    pub fn with_semantic_profile(&self, semantic_profile: P) -> Self {
        let mut data = (*self.signature.0).clone();
        data.semantic_profile = Arc::from(semantic_profile.as_ref());
        Self {
            signature: PlanSignature(Arc::new(data)),
            semantic_profile,
        }
    }

    /// Returns one unequal diagnostic variant for each structural field.
    ///
    /// This method is intended for identity-binding verification. The caller's
    /// profile default must differ from the active profile for all seven
    /// variants to be unequal.
    #[must_use]
    pub fn single_field_variants(&self) -> Vec<Self>
    where
        P: Default,
    {
        let mut variants = Vec::with_capacity(Self::FIELD_COUNT);

        let mut schema = (*self.signature.0).clone();
        schema.schema = schema.schema.wrapping_add(1);
        variants.push(self.with_signature(schema));

        let mut keys = (*self.signature.0).clone();
        if keys.keys.len() > 1 {
            keys.keys.swap(0, 1);
        } else {
            keys.schema = keys.schema.wrapping_add(1);
        }
        variants.push(self.with_signature(keys));

        let mut dependencies = (*self.signature.0).clone();
        if dependencies.dependencies.is_empty() {
            dependencies
                .dependencies
                .push((TaskId::new(0), TaskId::new(0)));
        } else {
            dependencies.dependencies.pop();
        }
        variants.push(self.with_signature(dependencies));

        let mut effects = (*self.signature.0).clone();
        if let Some((reads, _)) = effects.effects.first_mut() {
            if reads.last().copied() == Some(u64::MAX) {
                reads.pop();
            } else {
                reads.push(u64::MAX);
            }
        } else {
            effects.schema = effects.schema.wrapping_add(1);
        }
        variants.push(self.with_signature(effects));

        let mut costs = (*self.signature.0).clone();
        if let Some(cost) = costs.costs.first_mut() {
            *cost = cost.wrapping_add(1);
        } else {
            costs.schema = costs.schema.wrapping_add(1);
        }
        variants.push(self.with_signature(costs));

        variants.push(self.with_budget(self.budget().wrapping_add(1)));
        variants.push(self.with_semantic_profile(P::default()));
        variants
    }

    fn with_signature(&self, mut data: PlanSignatureData<K>) -> Self {
        data.semantic_profile = Arc::from(self.semantic_profile.as_ref());
        Self {
            signature: PlanSignature(Arc::new(data)),
            semantic_profile: self.semantic_profile.clone(),
        }
    }

    fn task_count(&self) -> usize {
        self.signature.0.keys.len()
    }

    fn task_key(&self, dense: usize) -> Option<&K> {
        self.signature.0.keys.get(dense)
    }

    fn dense(&self, key: &K) -> Option<TaskId> {
        self.signature
            .0
            .keys
            .binary_search(key)
            .ok()
            .and_then(|index| u32::try_from(index).ok())
            .map(TaskId::new)
    }

    fn input_cells(&self) -> u64 {
        let resources = self
            .signature
            .0
            .effects
            .iter()
            .map(|(reads, writes)| reads.len().saturating_add(writes.len()))
            .fold(0usize, usize::saturating_add);
        usize_to_u64(
            self.task_count()
                .saturating_add(self.signature.0.dependencies.len())
                .saturating_add(resources)
                .saturating_add(self.signature.0.costs.len()),
        )
    }
}

fn canonical_u64_set(values: Vec<u64>) -> Vec<u64> {
    if values.windows(2).all(|pair| pair[0] <= pair[1]) {
        let mut canonical = values;
        canonical.dedup();
        return canonical;
    }
    let mut heap = BinaryHeap::with_capacity(values.len());
    heap.extend(values.into_iter().map(Reverse));
    let mut canonical = Vec::with_capacity(heap.len());
    while let Some(Reverse(value)) = heap.pop() {
        if canonical.last().copied() != Some(value) {
            canonical.push(value);
        }
    }
    canonical
}

/// One-based durable event ordinal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct EventOrdinal(u64);

impl EventOrdinal {
    /// Returns the one-based integer representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum OutcomeKind<S, F, I> {
    Success(S),
    Failure(F),
    Incomplete(I),
    Cancelled,
    ResourceLimited,
    Completed,
}

/// A typed task or scheduling-boundary outcome.
///
/// The three type parameters independently preserve success, failure, and
/// incomplete payload types. The `PascalCase` constructors retained for the
/// protocol notation infer a homogeneous payload type; the lowercase
/// constructors support heterogeneous application payloads.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DurableOutcome<S = (), F = (), I = ()> {
    kind: OutcomeKind<S, F, I>,
}

impl<T> DurableOutcome<T, T, T> {
    /// Constructs a homogeneous typed success outcome.
    #[allow(non_snake_case)]
    #[must_use]
    pub fn Success(value: T) -> Self {
        Self {
            kind: OutcomeKind::Success(value),
        }
    }

    /// Constructs a homogeneous typed failure outcome.
    #[allow(non_snake_case)]
    #[must_use]
    pub fn Failure(value: T) -> Self {
        Self {
            kind: OutcomeKind::Failure(value),
        }
    }

    /// Constructs a homogeneous typed incomplete outcome.
    #[allow(non_snake_case)]
    #[must_use]
    pub fn Incomplete(value: T) -> Self {
        Self {
            kind: OutcomeKind::Incomplete(value),
        }
    }
}

impl<S, F, I> DurableOutcome<S, F, I> {
    /// Scheduling was cancelled at a pre-task boundary.
    #[allow(non_upper_case_globals)]
    pub const Cancelled: Self = Self {
        kind: OutcomeKind::Cancelled,
    };

    /// The resource policy rejected the next task.
    #[allow(non_upper_case_globals)]
    pub const ResourceLimited: Self = Self {
        kind: OutcomeKind::ResourceLimited,
    };

    /// Every task succeeded and the final boundary committed.
    #[allow(non_upper_case_globals)]
    pub const Completed: Self = Self {
        kind: OutcomeKind::Completed,
    };

    /// Constructs a success with an application-specific success type.
    #[must_use]
    pub fn success(value: S) -> Self {
        Self {
            kind: OutcomeKind::Success(value),
        }
    }

    /// Constructs a failure with an application-specific failure type.
    #[must_use]
    pub fn failure(value: F) -> Self {
        Self {
            kind: OutcomeKind::Failure(value),
        }
    }

    /// Constructs an incomplete outcome with its application-specific type.
    #[must_use]
    pub fn incomplete(value: I) -> Self {
        Self {
            kind: OutcomeKind::Incomplete(value),
        }
    }
}

/// Exact terminal classification retained by the protocol report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TerminalPhase {
    /// Execution is not yet terminal.
    Ready,
    /// Every planned task and the completion boundary committed.
    Completed,
    /// A task failed.
    Failed,
    /// A task returned an incomplete result.
    Incomplete,
    /// Cancellation won the next-task boundary.
    Cancelled,
    /// The resource policy rejected the next task.
    ResourceLimited,
    /// Input validation or replay safety rejected the run.
    Rejected,
}

impl TerminalPhase {
    /// Returns whether this phase permits no further protocol transition.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Ready)
    }
}

/// Stable identifier for one event within one exact structural plan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EventId<K> {
    plan: PlanSignature<K>,
    ordinal: EventOrdinal,
}

/// One typed append-only journal event.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DurableEvent<K, S, F, I> {
    plan: PlanSignature<K>,
    ordinal: EventOrdinal,
    task: Option<TaskId>,
    key: Option<K>,
    outcome: DurableOutcome<S, F, I>,
}

impl<K, S, F, I> DurableEvent<K, S, F, I>
where
    K: Clone + Ord,
{
    /// Constructs a task event tied to an exact structural plan.
    #[must_use]
    pub fn task<P>(
        plan: &PlanIdentity<K, P>,
        ordinal: u64,
        key: K,
        outcome: DurableOutcome<S, F, I>,
    ) -> Self
    where
        P: AsRef<str> + Clone + Eq,
    {
        let task = plan.dense(&key);
        Self {
            plan: plan.signature.clone(),
            ordinal: EventOrdinal(ordinal),
            task,
            key: Some(key),
            outcome,
        }
    }

    fn boundary<P>(
        plan: &PlanIdentity<K, P>,
        ordinal: u64,
        outcome: DurableOutcome<S, F, I>,
    ) -> Self
    where
        P: AsRef<str> + Clone + Eq,
    {
        Self {
            plan: plan.signature.clone(),
            ordinal: EventOrdinal(ordinal),
            task: None,
            key: None,
            outcome,
        }
    }

    /// Returns the exact plan-and-ordinal event identifier.
    #[must_use]
    pub fn id(&self) -> EventId<K> {
        EventId {
            plan: self.plan.clone(),
            ordinal: self.ordinal,
        }
    }

    /// Returns the one-based durable ordinal.
    #[must_use]
    pub const fn ordinal(&self) -> EventOrdinal {
        self.ordinal
    }

    /// Returns the exact terminal classification of this event.
    #[must_use]
    pub const fn terminal_phase(&self) -> TerminalPhase {
        match &self.outcome.kind {
            OutcomeKind::Success(_) => TerminalPhase::Ready,
            OutcomeKind::Failure(_) => TerminalPhase::Failed,
            OutcomeKind::Incomplete(_) => TerminalPhase::Incomplete,
            OutcomeKind::Cancelled => TerminalPhase::Cancelled,
            OutcomeKind::ResourceLimited => TerminalPhase::ResourceLimited,
            OutcomeKind::Completed => TerminalPhase::Completed,
        }
    }

    fn is_success(&self) -> bool {
        matches!(&self.outcome.kind, OutcomeKind::Success(_))
    }
}

/// Exact append-only event journal for one structural plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableJournal<K, S, F, I> {
    plan: PlanSignature<K>,
    events: Arc<Vec<DurableEvent<K, S, F, I>>>,
}

impl<K, S, F, I> DurableJournal<K, S, F, I>
where
    K: Clone + Ord,
    S: Clone + Eq,
    F: Clone + Eq,
    I: Clone + Eq,
{
    /// Creates an empty journal bound to `plan`.
    #[must_use]
    pub fn new<P>(plan: PlanIdentity<K, P>) -> Self
    where
        P: AsRef<str> + Clone + Eq,
    {
        Self {
            plan: plan.signature,
            events: Arc::new(Vec::new()),
        }
    }

    /// Returns the canonical event sequence.
    #[must_use]
    pub fn events(&self) -> &[DurableEvent<K, S, F, I>] {
        self.events.as_slice()
    }

    /// Returns the last event, if any.
    #[must_use]
    pub fn last(&self) -> Option<&DurableEvent<K, S, F, I>> {
        self.events.last()
    }

    /// Returns whether the journal ends in a terminal event.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.last()
            .is_some_and(|event| event.terminal_phase().is_terminal())
    }

    /// Returns whether `prefix` is an exact structural prefix of this journal.
    #[must_use]
    pub fn starts_with(&self, prefix: &Self) -> bool {
        self.plan == prefix.plan && self.events().starts_with(prefix.events())
    }

    /// Appends a terminal boundary event, rejecting a terminal journal.
    ///
    /// # Errors
    ///
    /// Returns [`DurableError::TerminalJournal`] after any terminal event, or
    /// [`DurableError::InvalidEvent`] for a success boundary.
    pub fn append_terminal(
        mut self,
        outcome: DurableOutcome<S, F, I>,
    ) -> Result<Self, DurableError> {
        if self.is_terminal() {
            return Err(DurableError::TerminalJournal);
        }
        let successes = self.successful_prefix_len();
        let valid_boundary = match &outcome.kind {
            OutcomeKind::Cancelled | OutcomeKind::ResourceLimited => {
                successes < self.plan.0.keys.len()
            }
            OutcomeKind::Completed => successes == self.plan.0.keys.len(),
            OutcomeKind::Success(_) | OutcomeKind::Failure(_) | OutcomeKind::Incomplete(_) => false,
        };
        if !valid_boundary {
            return Err(DurableError::InvalidEvent);
        }
        let event = DurableEvent {
            plan: self.plan.clone(),
            ordinal: EventOrdinal(usize_to_u64(self.events.len()).saturating_add(1)),
            task: None,
            key: None,
            outcome,
        };
        Arc::make_mut(&mut self.events).push(event);
        Ok(self)
    }

    fn append_checked(&mut self, event: DurableEvent<K, S, F, I>) -> Result<(), DurableError> {
        if self.is_terminal() {
            return Err(DurableError::TerminalJournal);
        }
        let expected = usize_to_u64(self.events.len()).saturating_add(1);
        if event.plan != self.plan || event.ordinal.get() != expected {
            return Err(DurableError::InvalidEvent);
        }
        Arc::make_mut(&mut self.events).push(event);
        Ok(())
    }

    fn successful_prefix_len(&self) -> usize {
        self.events()
            .iter()
            .take_while(|event| event.is_success())
            .count()
    }
}

/// Ordered, idempotent logical publication receipts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationLedger<K> {
    event_ids: Arc<Vec<EventId<K>>>,
}

impl<K> PublicationLedger<K>
where
    K: Clone + Ord,
{
    /// Creates an empty publication ledger.
    #[must_use]
    pub fn new() -> Self {
        Self {
            event_ids: Arc::new(Vec::new()),
        }
    }

    /// Publishes exactly the next durable event.
    ///
    /// Returns `false` for an already published event and `true` for a newly
    /// inserted receipt.
    ///
    /// # Errors
    ///
    /// Returns [`DurableError::PublicationOutOfOrder`] unless `event` is
    /// already published or is exactly the earliest unpublished journal event.
    pub fn publish<S, F, I>(
        &mut self,
        journal: &DurableJournal<K, S, F, I>,
        event: &DurableEvent<K, S, F, I>,
    ) -> Result<bool, DurableError>
    where
        S: Clone + Eq,
        F: Clone + Eq,
        I: Clone + Eq,
    {
        let identifier = event.id();
        let receipt_count = self.event_ids.len();
        let prior_index = usize::try_from(event.ordinal.get().saturating_sub(1)).ok();
        if let Some(existing) = prior_index.and_then(|index| self.event_ids.get(index)) {
            return if existing == &identifier {
                Ok(false)
            } else {
                Err(DurableError::PublicationOutOfOrder)
            };
        }
        let expected = journal.events().get(receipt_count).map(DurableEvent::id);
        if expected.as_ref() != Some(&identifier) {
            return Err(DurableError::PublicationOutOfOrder);
        }
        Arc::make_mut(&mut self.event_ids).push(identifier);
        Ok(true)
    }

    /// Returns the receipt count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.event_ids.len()
    }

    /// Returns whether the ledger contains no receipts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.event_ids.is_empty()
    }

    /// Returns canonical event identifiers in journal order.
    #[must_use]
    pub fn event_ids(&self) -> &[EventId<K>] {
        self.event_ids.as_slice()
    }

    /// Returns whether this receipt sequence is a prefix of `other`.
    #[must_use]
    pub fn is_subset_of(&self, other: &Self) -> bool {
        other.event_ids().starts_with(self.event_ids())
    }
}

impl<K> Default for PublicationLedger<K>
where
    K: Clone + Ord,
{
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckpointKind {
    Success,
    Failure,
    Incomplete,
    Cancelled,
    ResourceLimited,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CheckpointEvent<K> {
    ordinal: EventOrdinal,
    task: Option<TaskId>,
    key: Option<K>,
    kind: CheckpointKind,
}

/// Immutable durable recovery checkpoint.
///
/// Outcome payload Rust types are intentionally absent. The checkpoint stores
/// their typed variant and reconstructs payloads from the caller's immutable
/// outcome vector only after the complete checkpoint validates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint<K> {
    plan: StructuralPlanIdentity<K>,
    events: Arc<Vec<CheckpointEvent<K>>>,
    receipts: PublicationLedger<K>,
    next_task_cursor: usize,
    integrity_valid: bool,
    encoded_events: usize,
}

impl<K> Checkpoint<K>
where
    K: Clone + Ord,
{
    /// Creates a valid empty checkpoint for `plan`.
    #[must_use]
    pub fn empty<P>(plan: PlanIdentity<K, P>) -> Self
    where
        P: AsRef<str> + Clone + Eq,
    {
        Self {
            plan: StructuralPlanIdentity {
                signature: plan.signature,
            },
            events: Arc::new(Vec::new()),
            receipts: PublicationLedger::new(),
            next_task_cursor: 0,
            integrity_valid: true,
            encoded_events: 0,
        }
    }

    /// Creates a deliberately integrity-invalid checkpoint for rejection tests.
    #[must_use]
    pub fn corrupt<P>(plan: PlanIdentity<K, P>) -> Self
    where
        P: AsRef<str> + Clone + Eq,
    {
        let mut checkpoint = Self::empty(plan);
        checkpoint.integrity_valid = false;
        checkpoint
    }

    /// Returns representative single-field corruptions of `checkpoint`.
    #[must_use]
    pub fn malformed_variants(checkpoint: &Self) -> Vec<Self> {
        let mut variants = Vec::with_capacity(3);
        let mut integrity = checkpoint.clone();
        integrity.integrity_valid = false;
        variants.push(integrity);

        let mut cursor = checkpoint.clone();
        cursor.next_task_cursor = cursor.next_task_cursor.saturating_add(1);
        variants.push(cursor);

        let mut count = checkpoint.clone();
        count.encoded_events = count.encoded_events.saturating_add(1);
        variants.push(count);
        variants
    }

    /// Extracts the first `prefix` successful events and their receipts.
    ///
    /// # Errors
    ///
    /// Returns [`DurableError::MalformedCheckpoint`] when `prefix` exceeds the
    /// successful journal prefix.
    pub fn from_committed_prefix<S, F, I>(
        report: &ProtocolReport<K, S, F, I>,
        prefix: usize,
    ) -> Result<Self, DurableError>
    where
        S: Clone + Eq,
        F: Clone + Eq,
        I: Clone + Eq,
    {
        if prefix > report.journal.successful_prefix_len() {
            return Err(DurableError::MalformedCheckpoint);
        }
        Self::from_report_prefix(report, prefix, prefix)
    }

    /// Copies the complete journal while retaining only `published` receipts.
    ///
    /// # Errors
    ///
    /// Returns [`DurableError::MalformedCheckpoint`] when `published` exceeds
    /// the durable event count.
    pub fn with_published_prefix<S, F, I>(
        report: &ProtocolReport<K, S, F, I>,
        published: usize,
    ) -> Result<Self, DurableError>
    where
        S: Clone + Eq,
        F: Clone + Eq,
        I: Clone + Eq,
    {
        if published > report.journal.events.len() {
            return Err(DurableError::MalformedCheckpoint);
        }
        Self::from_report_prefix(report, report.journal.events.len(), published)
    }

    fn from_report_prefix<S, F, I>(
        report: &ProtocolReport<K, S, F, I>,
        events: usize,
        published: usize,
    ) -> Result<Self, DurableError>
    where
        S: Clone + Eq,
        F: Clone + Eq,
        I: Clone + Eq,
    {
        let checkpoint_events = report.journal.events()[..events]
            .iter()
            .map(|event| CheckpointEvent {
                ordinal: event.ordinal,
                task: event.task,
                key: event.key.clone(),
                kind: checkpoint_kind(&event.outcome),
            })
            .collect();
        let receipt_ids = report.receipts.event_ids()[..published].to_vec();
        let next_task_cursor = report.journal.events()[..events]
            .iter()
            .take_while(|event| event.is_success())
            .count();
        let checkpoint = Self {
            plan: report.plan.clone(),
            events: Arc::new(checkpoint_events),
            receipts: PublicationLedger {
                event_ids: Arc::new(receipt_ids),
            },
            next_task_cursor,
            integrity_valid: true,
            encoded_events: events,
        };
        checkpoint.validate_structural()?;
        Ok(checkpoint)
    }

    /// Validates every structural checkpoint field against `active`.
    ///
    /// # Errors
    ///
    /// Returns [`DurableError::ForeignPlan`] for an unequal structural plan or
    /// [`DurableError::MalformedCheckpoint`] for any inconsistent field.
    pub fn validate_for<P>(&self, active: &PlanIdentity<K, P>) -> Result<(), DurableError>
    where
        P: AsRef<str> + Clone + Eq,
    {
        if &self.plan != active {
            return Err(DurableError::ForeignPlan);
        }
        self.validate_structural()
    }

    fn validate_structural(&self) -> Result<(), DurableError> {
        if !self.integrity_valid
            || self.encoded_events != self.events.len()
            || self.next_task_cursor != checkpoint_successful_prefix_len(&self.events)
            || !checkpoint_events_are_canonical(&self.plan, &self.events)
            || !checkpoint_receipts_are_prefix(&self.plan, &self.events, &self.receipts)
        {
            return Err(DurableError::MalformedCheckpoint);
        }
        Ok(())
    }

    /// Returns the reconstructed zero-based next-task cursor.
    #[must_use]
    pub const fn next_task_cursor(&self) -> usize {
        self.next_task_cursor
    }

    fn materialize<P, S, F, I>(
        &self,
        active: &PlanIdentity<K, P>,
        outcomes: &[DurableOutcome<S, F, I>],
    ) -> Result<DurableJournal<K, S, F, I>, DurableError>
    where
        P: AsRef<str> + Clone + Eq,
        S: Clone + Eq,
        F: Clone + Eq,
        I: Clone + Eq,
    {
        let mut journal = DurableJournal {
            plan: active.signature.clone(),
            events: Arc::new(Vec::with_capacity(active.task_count().saturating_add(1))),
        };
        for checkpoint_event in self.events.iter() {
            let outcome = match checkpoint_event.kind {
                CheckpointKind::Success => {
                    outcome_for_kind(outcomes, checkpoint_event.task, CheckpointKind::Success)?
                }
                CheckpointKind::Failure => {
                    outcome_for_kind(outcomes, checkpoint_event.task, CheckpointKind::Failure)?
                }
                CheckpointKind::Incomplete => {
                    outcome_for_kind(outcomes, checkpoint_event.task, CheckpointKind::Incomplete)?
                }
                CheckpointKind::Cancelled => DurableOutcome::Cancelled,
                CheckpointKind::ResourceLimited => DurableOutcome::ResourceLimited,
                CheckpointKind::Completed => DurableOutcome::Completed,
            };
            journal.append_checked(DurableEvent {
                plan: active.signature.clone(),
                ordinal: checkpoint_event.ordinal,
                task: checkpoint_event.task,
                key: checkpoint_event.key.clone(),
                outcome,
            })?;
        }
        Ok(journal)
    }
}

fn checkpoint_kind<S, F, I>(outcome: &DurableOutcome<S, F, I>) -> CheckpointKind {
    match &outcome.kind {
        OutcomeKind::Success(_) => CheckpointKind::Success,
        OutcomeKind::Failure(_) => CheckpointKind::Failure,
        OutcomeKind::Incomplete(_) => CheckpointKind::Incomplete,
        OutcomeKind::Cancelled => CheckpointKind::Cancelled,
        OutcomeKind::ResourceLimited => CheckpointKind::ResourceLimited,
        OutcomeKind::Completed => CheckpointKind::Completed,
    }
}

fn checkpoint_successful_prefix_len<K>(events: &[CheckpointEvent<K>]) -> usize {
    events
        .iter()
        .take_while(|event| event.kind == CheckpointKind::Success)
        .count()
}

fn checkpoint_events_are_canonical<K: Ord>(
    plan: &StructuralPlanIdentity<K>,
    events: &[CheckpointEvent<K>],
) -> bool {
    let task_count = plan.signature.0.keys.len();
    if events.len() > task_count.saturating_add(1) {
        return false;
    }
    let mut success_count = 0usize;
    let last_index = events.len().saturating_sub(1);
    for (index, event) in events.iter().enumerate() {
        if event.ordinal.get() != usize_to_u64(index) + 1 {
            return false;
        }
        match event.kind {
            CheckpointKind::Success => {
                if event.task.map(TaskId::index) != Some(success_count)
                    || event.key.as_ref() != plan.signature.0.keys.get(success_count)
                {
                    return false;
                }
                success_count += 1;
            }
            CheckpointKind::Failure | CheckpointKind::Incomplete => {
                if index != last_index
                    || success_count >= task_count
                    || event.task.map(TaskId::index) != Some(success_count)
                    || event.key.as_ref() != plan.signature.0.keys.get(success_count)
                {
                    return false;
                }
            }
            CheckpointKind::Cancelled | CheckpointKind::ResourceLimited => {
                if index != last_index
                    || success_count >= task_count
                    || event.task.is_some()
                    || event.key.is_some()
                {
                    return false;
                }
            }
            CheckpointKind::Completed => {
                if index != last_index
                    || success_count != task_count
                    || event.task.is_some()
                    || event.key.is_some()
                {
                    return false;
                }
            }
        }
    }
    true
}

fn checkpoint_receipts_are_prefix<K: Clone + Ord>(
    plan: &StructuralPlanIdentity<K>,
    events: &[CheckpointEvent<K>],
    receipts: &PublicationLedger<K>,
) -> bool {
    receipts.len() <= events.len()
        && events
            .iter()
            .take(receipts.len())
            .map(|event| EventId {
                plan: plan.signature.clone(),
                ordinal: event.ordinal,
            })
            .eq(receipts.event_ids.iter().cloned())
}

fn outcome_for_kind<S, F, I>(
    outcomes: &[DurableOutcome<S, F, I>],
    task: Option<TaskId>,
    expected: CheckpointKind,
) -> Result<DurableOutcome<S, F, I>, DurableError>
where
    S: Clone,
    F: Clone,
    I: Clone,
{
    let outcome = task
        .and_then(|task| outcomes.get(task.index()))
        .ok_or(DurableError::MalformedCheckpoint)?;
    if checkpoint_kind(outcome) != expected {
        return Err(DurableError::MalformedCheckpoint);
    }
    Ok(outcome.clone())
}

/// Explicit witness governing pre-journal task replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayClass {
    /// Equal input deterministically reproduces equal effects and result.
    Deterministic,
    /// Repeating the external effect is observationally idempotent.
    Idempotent,
    /// An external atomic retry or deduplication boundary exists.
    Transactional,
    /// No replay-safety witness exists.
    Unsafe,
}

impl ReplayClass {
    fn admits_replay(self) -> bool {
        !matches!(self, Self::Unsafe)
    }
}

/// A deterministic crash boundary in the durable transition protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CrashPoint {
    /// Crash after computation but before journal append.
    BeforeJournal {
        /// One-based event ordinal.
        ordinal: u64,
    },
    /// Crash after journal append but before logical publication.
    AfterJournal {
        /// One-based event ordinal.
        ordinal: u64,
    },
    /// Crash after logical publication but before cursor advance.
    AfterPublication {
        /// One-based event ordinal.
        ordinal: u64,
    },
}

impl CrashPoint {
    /// Returns one crash at each transition boundary for the first event.
    #[must_use]
    pub fn all_single_crashes() -> Vec<Self> {
        vec![
            Self::BeforeJournal { ordinal: 1 },
            Self::AfterJournal { ordinal: 1 },
            Self::AfterPublication { ordinal: 1 },
        ]
    }

    /// Returns a bounded, deterministic crash schedule over `event_count`.
    #[must_use]
    pub fn all_bounded_subsets(event_count: u64, depth: usize) -> Vec<Self> {
        let capacity = usize::try_from(event_count)
            .unwrap_or(usize::MAX)
            .saturating_mul(depth.min(3));
        let mut points = Vec::with_capacity(capacity);
        for ordinal in 1..=event_count {
            if depth >= 1 {
                points.push(Self::BeforeJournal { ordinal });
            }
            if depth >= 2 {
                points.push(Self::AfterJournal { ordinal });
            }
            if depth >= 3 {
                points.push(Self::AfterPublication { ordinal });
            }
        }
        points
    }

    const fn ordinal(self) -> u64 {
        match self {
            Self::BeforeJournal { ordinal }
            | Self::AfterJournal { ordinal }
            | Self::AfterPublication { ordinal } => ordinal,
        }
    }

    const fn slot(self) -> usize {
        match self {
            Self::BeforeJournal { .. } => 0,
            Self::AfterJournal { .. } => 1,
            Self::AfterPublication { .. } => 2,
        }
    }
}

/// A bounded codec usage counter that performs checks before allocation.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CodecUsage {
    events: usize,
    bytes: usize,
}

impl CodecUsage {
    /// Returns the accounted event count.
    #[must_use]
    pub const fn events(self) -> usize {
        self.events
    }

    /// Returns the accounted byte count.
    #[must_use]
    pub const fn bytes(self) -> usize {
        self.bytes
    }
}

/// Hard event and byte limits for a future portable codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodecLimits {
    events: usize,
    bytes: usize,
}

impl CodecLimits {
    /// Creates an immutable codec limit pair.
    #[must_use]
    pub const fn new(events: usize, bytes: usize) -> Self {
        Self { events, bytes }
    }

    /// Checks one append before returning the next usage state.
    ///
    /// # Errors
    ///
    /// Returns [`DurableError::CodecLimitExceeded`] before allocation when the
    /// event count, byte count, or checked arithmetic exceeds its bound.
    pub fn checked_append(
        self,
        usage: CodecUsage,
        event_bytes: usize,
    ) -> Result<CodecUsage, DurableError> {
        let events = usage
            .events
            .checked_add(1)
            .ok_or(DurableError::CodecLimitExceeded)?;
        let bytes = usage
            .bytes
            .checked_add(event_bytes)
            .ok_or(DurableError::CodecLimitExceeded)?;
        if events > self.events || bytes > self.bytes {
            return Err(DurableError::CodecLimitExceeded);
        }
        Ok(CodecUsage { events, bytes })
    }

    /// Returns rejected allocation attempts; checks reject before allocation.
    #[must_use]
    pub const fn rejected_allocation_count(self) -> usize {
        0
    }
}

/// Internal and terminal phases visible in immutable protocol snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolPhase {
    /// Validating all immutable inputs before task effects.
    Validate,
    /// Ready to publish a durable event or stage canonical work.
    Ready,
    /// A task or boundary event has been staged.
    Computed,
    /// The staged event has been appended durably.
    Journaled,
    /// The next durable event has been published logically.
    Publishing,
    /// Volatile staging was discarded while durable prefixes survived.
    Crashed,
    /// The journal terminated successfully.
    Completed,
    /// The journal terminated with failure.
    Failed,
    /// The journal terminated incomplete.
    Incomplete,
    /// The journal terminated through cancellation.
    Cancelled,
    /// The journal terminated at a resource boundary.
    ResourceLimited,
    /// Validation or unsafe replay rejected execution.
    Rejected,
}

/// Shared immutable journal prefix captured at one protocol transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalSnapshot<K, S, F, I> {
    plan: PlanSignature<K>,
    events: Arc<Vec<DurableEvent<K, S, F, I>>>,
    len: usize,
}

impl<K, S, F, I> JournalSnapshot<K, S, F, I>
where
    K: Clone + Ord,
    S: Clone + Eq,
    F: Clone + Eq,
    I: Clone + Eq,
{
    /// Returns whether `prefix` is an exact prefix of this snapshot.
    #[must_use]
    pub fn starts_with(&self, prefix: &Self) -> bool {
        self.plan == prefix.plan
            && self.len >= prefix.len
            && self.events[..self.len].starts_with(&prefix.events[..prefix.len])
    }
}

/// Shared immutable publication-receipt prefix at one transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationSnapshot<K> {
    event_ids: Arc<Vec<EventId<K>>>,
    len: usize,
}

impl<K: Ord> PublicationSnapshot<K> {
    /// Returns whether this ordered receipt prefix is contained by `other`.
    #[must_use]
    pub fn is_subset_of(&self, other: &Self) -> bool {
        other.len >= self.len
            && other.event_ids[..other.len].starts_with(&self.event_ids[..self.len])
    }
}

/// An immutable state observation backed by shared final protocol storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolSnapshot<K, S, F, I> {
    plan: StructuralPlanIdentity<K>,
    phase: ProtocolPhase,
    cursor: usize,
    journal: JournalSnapshot<K, S, F, I>,
    receipts: PublicationSnapshot<K>,
    remaining_replay_events: usize,
    point: Option<CrashPoint>,
}

impl<K, S, F, I> ProtocolSnapshot<K, S, F, I>
where
    K: Clone + Ord,
    S: Clone + Eq,
    F: Clone + Eq,
    I: Clone + Eq,
{
    /// Returns whether all snapshot fields inhabit their finite typed domains.
    #[must_use]
    pub fn is_well_typed(&self) -> bool {
        self.cursor <= self.plan.task_count()
            && self.journal.len <= self.journal.events.len()
            && self.receipts.len <= self.receipts.event_ids.len()
            && self.receipts.len <= self.journal.len
    }

    /// Returns the exact immutable structural plan identity.
    #[must_use]
    pub const fn plan_identity(&self) -> &StructuralPlanIdentity<K> {
        &self.plan
    }

    /// Returns the protocol phase.
    #[must_use]
    pub const fn phase(&self) -> ProtocolPhase {
        self.phase
    }

    /// Returns the durable journal prefix at this transition.
    #[must_use]
    pub const fn journal(&self) -> &JournalSnapshot<K, S, F, I> {
        &self.journal
    }

    /// Returns the publication-receipt prefix at this transition.
    #[must_use]
    pub const fn receipts(&self) -> &PublicationSnapshot<K> {
        &self.receipts
    }

    /// Returns the explicit replay measure retained by this snapshot.
    #[must_use]
    pub const fn remaining_replay_events(&self) -> usize {
        self.remaining_replay_events
    }

    /// Models a crash by discarding volatile phase while preserving prefixes.
    #[must_use]
    pub fn crash(&self) -> Self {
        let mut crashed = self.clone();
        crashed.phase = ProtocolPhase::Crashed;
        crashed
    }
}

#[derive(Debug, Clone, Copy)]
struct SnapshotDraft {
    phase: ProtocolPhase,
    cursor: usize,
    journal_len: usize,
    receipt_len: usize,
    point: Option<CrashPoint>,
}

/// Auditable resource accounting for one recovery run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurableWorkProfile {
    steps: u64,
    input_cells: u64,
    heap_cells: u64,
    maximum_native_frames: u64,
}

impl DurableWorkProfile {
    /// Returns state-machine loop iterations.
    #[must_use]
    pub const fn steps(self) -> u64 {
        self.steps
    }

    /// Returns the finite cells supplied to validation and recovery.
    #[must_use]
    pub const fn input_cells(self) -> u64 {
        self.input_cells
    }

    /// Returns the proven upper bound on heap-resident protocol cells.
    #[must_use]
    pub const fn heap_cells(self) -> u64 {
        self.heap_cells
    }

    /// Returns the input-independent native-frame bound.
    #[must_use]
    pub const fn maximum_native_frames(self) -> u64 {
        self.maximum_native_frames
    }

    /// Returns the formally established finite transition bound.
    #[must_use]
    pub const fn proven_step_bound(self) -> u64 {
        self.input_cells.saturating_add(1).saturating_mul(32)
    }
}

/// Successful terminal result of durable recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolReport<K, S, F, I> {
    plan: StructuralPlanIdentity<K>,
    phase: TerminalPhase,
    journal: DurableJournal<K, S, F, I>,
    receipts: PublicationLedger<K>,
    snapshots: Vec<ProtocolSnapshot<K, S, F, I>>,
    work: DurableWorkProfile,
}

impl<K, S, F, I> ProtocolReport<K, S, F, I>
where
    K: Clone + Ord,
    S: Clone + Eq,
    F: Clone + Eq,
    I: Clone + Eq,
{
    /// Returns the exact terminal phase.
    #[must_use]
    pub const fn phase(&self) -> TerminalPhase {
        self.phase
    }

    /// Returns the final authoritative journal.
    #[must_use]
    pub const fn journal(&self) -> &DurableJournal<K, S, F, I> {
        &self.journal
    }

    /// Returns final ordered publication receipts.
    #[must_use]
    pub const fn receipts(&self) -> &PublicationLedger<K> {
        &self.receipts
    }

    /// Returns the deduplicated logical observation in event order.
    #[must_use]
    pub fn observation(&self) -> &[EventId<K>] {
        self.receipts.event_ids()
    }

    /// Returns immutable transition snapshots.
    #[must_use]
    pub fn snapshots(&self) -> &[ProtocolSnapshot<K, S, F, I>] {
        &self.snapshots
    }

    /// Returns the snapshot immediately before the requested crash boundary.
    #[must_use]
    pub fn snapshot_before(&self, point: CrashPoint) -> Option<&ProtocolSnapshot<K, S, F, I>> {
        self.snapshots
            .iter()
            .find(|snapshot| snapshot.point == Some(point))
    }

    /// Returns resource-accounting evidence.
    #[must_use]
    pub const fn work(&self) -> DurableWorkProfile {
        self.work
    }
}

/// Rejected protocol result, including the unchanged durable prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError<K, S, F, I> {
    reason: DurableError,
    journal: DurableJournal<K, S, F, I>,
    observation: Vec<EventId<K>>,
    started_tasks: usize,
}

impl<K, S, F, I> ProtocolError<K, S, F, I> {
    /// Returns the rejection reason.
    #[must_use]
    pub const fn reason(&self) -> DurableError {
        self.reason
    }

    /// Returns the unchanged authoritative durable journal.
    #[must_use]
    pub const fn journal(&self) -> &DurableJournal<K, S, F, I> {
        &self.journal
    }

    /// Returns logical observations made before rejection.
    #[must_use]
    pub fn observation(&self) -> &[EventId<K>] {
        &self.observation
    }

    /// Returns the number of task executions started before rejection.
    #[must_use]
    pub const fn started_tasks(&self) -> usize {
        self.started_tasks
    }
}

/// Immutable input to the durable recovery machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolInput<K = (), P = (), S = (), F = (), I = ()> {
    plan: PlanIdentity<K, P>,
    outcomes: Vec<DurableOutcome<S, F, I>>,
    checkpoint: Checkpoint<K>,
    replay_classes: Vec<ReplayClass>,
    crashes: Vec<CrashPoint>,
    completion_order: Option<Vec<usize>>,
    cancel_after: Option<usize>,
    resource_after: Option<usize>,
    configuration_valid: bool,
}

impl<K, P, S, F, I> ProtocolInput<K, P, S, F, I>
where
    K: Clone + Ord,
    P: AsRef<str> + Clone + Eq,
    S: Clone + Eq,
    F: Clone + Eq,
    I: Clone + Eq,
{
    /// Starts from an empty checkpoint with deterministic replay witnesses.
    #[must_use]
    pub fn new(plan: PlanIdentity<K, P>, outcomes: Vec<DurableOutcome<S, F, I>>) -> Self {
        let task_count = plan.task_count();
        Self {
            checkpoint: Checkpoint::empty(plan.clone()),
            plan,
            outcomes,
            replay_classes: vec![ReplayClass::Deterministic; task_count],
            crashes: Vec::new(),
            completion_order: None,
            cancel_after: None,
            resource_after: None,
            configuration_valid: true,
        }
    }

    /// Resumes an existing checkpoint under an exact active plan.
    #[must_use]
    pub fn resume(
        plan: PlanIdentity<K, P>,
        outcomes: Vec<DurableOutcome<S, F, I>>,
        checkpoint: Checkpoint<K>,
    ) -> Self {
        let task_count = plan.task_count();
        Self {
            plan,
            outcomes,
            checkpoint,
            replay_classes: vec![ReplayClass::Deterministic; task_count],
            crashes: Vec::new(),
            completion_order: None,
            cancel_after: None,
            resource_after: None,
            configuration_valid: true,
        }
    }

    /// Assigns an explicit replay class to one caller-owned task key.
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn with_replay_class(mut self, key: K, replay_class: ReplayClass) -> Self {
        if let Some(dense) = self.plan.dense(&key) {
            self.replay_classes[dense.index()] = replay_class;
        } else {
            self.configuration_valid = false;
        }
        self
    }

    /// Adds deterministic crash points; each point is consumed at most once.
    #[must_use]
    pub fn with_crashes(mut self, crashes: impl IntoIterator<Item = CrashPoint>) -> Self {
        self.crashes.extend(crashes);
        self
    }

    /// Supplies a physical worker completion permutation.
    #[must_use]
    pub fn with_completion_order(mut self, order: impl IntoIterator<Item = usize>) -> Self {
        self.completion_order = Some(order.into_iter().collect());
        self
    }

    /// Requests cancellation at a zero-based pre-task boundary.
    #[must_use]
    pub fn with_cancellation_after(mut self, successful_tasks: usize) -> Self {
        self.cancel_after = Some(successful_tasks);
        self
    }

    /// Requests resource limitation at a zero-based pre-task boundary.
    #[must_use]
    pub fn with_resource_limit_after(mut self, successful_tasks: usize) -> Self {
        self.resource_after = Some(successful_tasks);
        self
    }
}

impl ProtocolInput<(), (), (), (), ()> {
    /// Enumerates physical completion permutations without recursion.
    #[must_use]
    pub fn completion_permutations(task_count: usize) -> CompletionPermutations {
        CompletionPermutations::new(task_count)
    }
}

impl<K, P, S, F, I> ProtocolInput<K, P, S, F, I>
where
    K: Clone + Ord,
    P: AsRef<str> + Clone + Eq,
    S: Clone + Eq + Default,
    F: Clone + Eq,
    I: Clone + Eq,
{
    /// Builds the smallest canonical run ending in the requested outcome.
    #[must_use]
    pub fn single_terminal(plan: PlanIdentity<K, P>, outcome: DurableOutcome<S, F, I>) -> Self {
        let task_count = plan.task_count();
        let mut outcomes: Vec<_> = (0..task_count)
            .map(|_| DurableOutcome::success(S::default()))
            .collect();
        let mut input = Self::new(plan, outcomes.clone());
        match outcome.kind {
            OutcomeKind::Success(success) => {
                if let Some(first) = outcomes.first_mut() {
                    *first = DurableOutcome::success(success);
                }
                input.outcomes = outcomes;
            }
            OutcomeKind::Failure(failure) => {
                if let Some(first) = outcomes.first_mut() {
                    *first = DurableOutcome::failure(failure);
                }
                input.outcomes = outcomes;
            }
            OutcomeKind::Incomplete(incomplete) => {
                if let Some(first) = outcomes.first_mut() {
                    *first = DurableOutcome::incomplete(incomplete);
                }
                input.outcomes = outcomes;
            }
            OutcomeKind::Cancelled => input.cancel_after = Some(0),
            OutcomeKind::ResourceLimited => input.resource_after = Some(0),
            OutcomeKind::Completed => {}
        }
        input
    }
}

/// Iterative lexicographic permutation generator.
#[derive(Debug, Clone)]
pub struct CompletionPermutations {
    current: Vec<usize>,
    first: bool,
    finished: bool,
}

impl CompletionPermutations {
    fn new(task_count: usize) -> Self {
        Self {
            current: (0..task_count).collect(),
            first: true,
            finished: false,
        }
    }
}

impl Iterator for CompletionPermutations {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        if self.first {
            self.first = false;
            return Some(self.current.clone());
        }
        if self.current.len() < 2 {
            self.finished = true;
            return None;
        }

        let mut pivot = self.current.len() - 2;
        while self.current[pivot] >= self.current[pivot + 1] {
            if pivot == 0 {
                self.finished = true;
                return None;
            }
            pivot -= 1;
        }
        let mut successor = self.current.len() - 1;
        while self.current[successor] <= self.current[pivot] {
            successor -= 1;
        }
        self.current.swap(pivot, successor);
        self.current[pivot + 1..].reverse();
        Some(self.current.clone())
    }
}

/// Iterative refinement of the verified committed-prefix state machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResumeMachine;

/// Terminal success-or-rejection result of one durable recovery run.
pub type ProtocolResult<K, S, F, I> = Result<ProtocolReport<K, S, F, I>, ProtocolError<K, S, F, I>>;

impl ResumeMachine {
    /// Validates and executes one finite recovery input.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] without starting new effects for invalid
    /// inputs, or after preserving the authoritative durable prefix when an
    /// unsafe replay or bounded transition failure is detected.
    #[allow(clippy::too_many_lines)]
    pub fn run<K, P, S, F, I>(input: ProtocolInput<K, P, S, F, I>) -> ProtocolResult<K, S, F, I>
    where
        K: Clone + Ord,
        P: AsRef<str> + Clone + Eq,
        S: Clone + Eq,
        F: Clone + Eq,
        I: Clone + Eq,
    {
        let ProtocolInput {
            plan,
            outcomes,
            checkpoint,
            replay_classes,
            crashes,
            completion_order,
            cancel_after,
            resource_after,
            configuration_valid,
        } = input;
        let mut started_tasks = 0usize;
        let empty_journal = DurableJournal {
            plan: plan.signature.clone(),
            events: Arc::new(Vec::new()),
        };

        if let Err(reason) = checkpoint.validate_for(&plan) {
            return Err(protocol_error(
                reason,
                empty_journal,
                PublicationLedger::new(),
                0,
            ));
        }
        if outcomes.len() != plan.task_count() {
            return Err(protocol_error(
                DurableError::OutcomeCountMismatch,
                empty_journal,
                PublicationLedger::new(),
                0,
            ));
        }
        let event_capacity = plan.task_count().saturating_add(1);
        if !configuration_valid
            || crashes
                .iter()
                .any(|point| point.ordinal() == 0 || point.ordinal() > usize_to_u64(event_capacity))
        {
            return Err(protocol_error(
                DurableError::InvalidProtocolConfiguration,
                empty_journal,
                PublicationLedger::new(),
                0,
            ));
        }
        let mut journal = match checkpoint.materialize(&plan, &outcomes) {
            Ok(journal) => journal,
            Err(reason) => {
                return Err(protocol_error(
                    reason,
                    empty_journal,
                    PublicationLedger::new(),
                    0,
                ));
            }
        };
        let mut receipts = checkpoint.receipts.clone();
        if replay_classes.len() != plan.task_count()
            || !completion_order_is_valid(completion_order.as_deref(), plan.task_count())
        {
            return Err(protocol_error(
                DurableError::InvalidCompletionOrder,
                journal,
                receipts,
                0,
            ));
        }

        let input_cells = plan
            .input_cells()
            .saturating_add(usize_to_u64(checkpoint.encoded_events))
            .saturating_add(usize_to_u64(checkpoint.receipts.len()))
            .saturating_add(usize_to_u64(crashes.len()))
            .saturating_add(usize_to_u64(completion_order.as_ref().map_or(0, Vec::len)));
        let step_bound = input_cells.saturating_add(1).saturating_mul(32);
        let additional_events = event_capacity.saturating_sub(journal.events.len());
        let additional_receipts = event_capacity.saturating_sub(receipts.event_ids.len());
        Arc::make_mut(&mut journal.events).reserve(additional_events);
        Arc::make_mut(&mut receipts.event_ids).reserve(additional_receipts);
        let mut crash_requested = vec![[false; 3]; event_capacity];
        for point in crashes {
            let ordinal = point.ordinal();
            if ordinal > 0 {
                if let Ok(index) = usize::try_from(ordinal - 1) {
                    if let Some(row) = crash_requested.get_mut(index) {
                        row[point.slot()] = true;
                    }
                }
            }
        }
        let mut crash_consumed = vec![[false; 3]; event_capacity];
        let snapshot_capacity = event_capacity.saturating_mul(4).saturating_add(2);
        let mut drafts = Vec::with_capacity(snapshot_capacity);
        drafts.push(SnapshotDraft {
            phase: ProtocolPhase::Validate,
            cursor: checkpoint.next_task_cursor,
            journal_len: journal.events.len(),
            receipt_len: receipts.len(),
            point: None,
        });
        let mut cursor = journal.successful_prefix_len();
        drafts.push(SnapshotDraft {
            phase: ProtocolPhase::Ready,
            cursor,
            journal_len: journal.events.len(),
            receipt_len: receipts.len(),
            point: None,
        });

        let mut steps = 0u64;
        loop {
            if steps >= step_bound {
                return Err(protocol_error(
                    DurableError::StepBoundExceeded,
                    journal,
                    receipts,
                    started_tasks,
                ));
            }
            steps += 1;

            if receipts.len() < journal.events.len() {
                let event = journal.events()[receipts.len()].clone();
                let point = CrashPoint::AfterPublication {
                    ordinal: event.ordinal.get(),
                };
                if receipts.publish(&journal, &event).is_err() {
                    return Err(protocol_error(
                        DurableError::PublicationOutOfOrder,
                        journal,
                        receipts,
                        started_tasks,
                    ));
                }
                drafts.push(SnapshotDraft {
                    phase: ProtocolPhase::Publishing,
                    cursor,
                    journal_len: journal.events.len(),
                    receipt_len: receipts.len(),
                    point: Some(point),
                });
                cursor = journal.successful_prefix_len();
                continue;
            }

            cursor = journal.successful_prefix_len();
            if let Some(last) = journal.last() {
                let terminal = last.terminal_phase();
                if terminal.is_terminal() {
                    drafts.push(SnapshotDraft {
                        phase: protocol_phase(terminal),
                        cursor,
                        journal_len: journal.events.len(),
                        receipt_len: receipts.len(),
                        point: None,
                    });
                    return Ok(finish_report(
                        &plan,
                        terminal,
                        journal,
                        receipts,
                        drafts,
                        steps,
                        input_cells,
                    ));
                }
            }

            let outcome;
            let event = if cursor == plan.task_count() {
                DurableEvent::boundary(
                    &plan,
                    usize_to_u64(cursor).saturating_add(1),
                    DurableOutcome::Completed,
                )
            } else if cancel_after == Some(cursor) {
                DurableEvent::boundary(
                    &plan,
                    usize_to_u64(cursor).saturating_add(1),
                    DurableOutcome::Cancelled,
                )
            } else if resource_after == Some(cursor) {
                DurableEvent::boundary(
                    &plan,
                    usize_to_u64(cursor).saturating_add(1),
                    DurableOutcome::ResourceLimited,
                )
            } else {
                outcome = outcomes[cursor].clone();
                let Some(task_key) = plan.task_key(cursor).cloned() else {
                    return Err(protocol_error(
                        DurableError::InvalidEvent,
                        journal,
                        receipts,
                        started_tasks,
                    ));
                };
                match outcome.kind {
                    OutcomeKind::Success(success) => {
                        started_tasks = started_tasks.saturating_add(1);
                        DurableEvent::task(
                            &plan,
                            usize_to_u64(cursor).saturating_add(1),
                            task_key,
                            DurableOutcome::success(success),
                        )
                    }
                    OutcomeKind::Failure(failure) => {
                        started_tasks = started_tasks.saturating_add(1);
                        DurableEvent::task(
                            &plan,
                            usize_to_u64(cursor).saturating_add(1),
                            task_key,
                            DurableOutcome::failure(failure),
                        )
                    }
                    OutcomeKind::Incomplete(incomplete) => {
                        started_tasks = started_tasks.saturating_add(1);
                        DurableEvent::task(
                            &plan,
                            usize_to_u64(cursor).saturating_add(1),
                            task_key,
                            DurableOutcome::incomplete(incomplete),
                        )
                    }
                    OutcomeKind::Cancelled => DurableEvent::boundary(
                        &plan,
                        usize_to_u64(cursor).saturating_add(1),
                        DurableOutcome::Cancelled,
                    ),
                    OutcomeKind::ResourceLimited => DurableEvent::boundary(
                        &plan,
                        usize_to_u64(cursor).saturating_add(1),
                        DurableOutcome::ResourceLimited,
                    ),
                    OutcomeKind::Completed => {
                        return Err(protocol_error(
                            DurableError::InvalidEvent,
                            journal,
                            receipts,
                            started_tasks,
                        ));
                    }
                }
            };
            let ordinal = event.ordinal.get();
            let before = CrashPoint::BeforeJournal { ordinal };
            drafts.push(SnapshotDraft {
                phase: ProtocolPhase::Computed,
                cursor,
                journal_len: journal.events.len(),
                receipt_len: receipts.len(),
                point: Some(before),
            });
            if consume_crash(before, &crash_requested, &mut crash_consumed) {
                if event.task.is_some() && !replay_classes[cursor].admits_replay() {
                    return Err(protocol_error(
                        DurableError::UnsafeReplay,
                        journal,
                        receipts,
                        started_tasks,
                    ));
                }
                drafts.push(SnapshotDraft {
                    phase: ProtocolPhase::Crashed,
                    cursor,
                    journal_len: journal.events.len(),
                    receipt_len: receipts.len(),
                    point: None,
                });
                continue;
            }

            if journal.append_checked(event.clone()).is_err() {
                return Err(protocol_error(
                    DurableError::InvalidEvent,
                    journal,
                    receipts,
                    started_tasks,
                ));
            }
            let after_journal = CrashPoint::AfterJournal { ordinal };
            drafts.push(SnapshotDraft {
                phase: ProtocolPhase::Journaled,
                cursor,
                journal_len: journal.events.len(),
                receipt_len: receipts.len(),
                point: Some(after_journal),
            });
            if consume_crash(after_journal, &crash_requested, &mut crash_consumed) {
                drafts.push(SnapshotDraft {
                    phase: ProtocolPhase::Crashed,
                    cursor,
                    journal_len: journal.events.len(),
                    receipt_len: receipts.len(),
                    point: None,
                });
                continue;
            }

            if receipts.publish(&journal, &event).is_err() {
                return Err(protocol_error(
                    DurableError::PublicationOutOfOrder,
                    journal,
                    receipts,
                    started_tasks,
                ));
            }
            let after_publication = CrashPoint::AfterPublication { ordinal };
            drafts.push(SnapshotDraft {
                phase: ProtocolPhase::Publishing,
                cursor,
                journal_len: journal.events.len(),
                receipt_len: receipts.len(),
                point: Some(after_publication),
            });
            if consume_crash(after_publication, &crash_requested, &mut crash_consumed) {
                drafts.push(SnapshotDraft {
                    phase: ProtocolPhase::Crashed,
                    cursor,
                    journal_len: journal.events.len(),
                    receipt_len: receipts.len(),
                    point: None,
                });
            }
        }
    }
}

fn completion_order_is_valid(order: Option<&[usize]>, task_count: usize) -> bool {
    let Some(order) = order else {
        return true;
    };
    if order.len() != task_count {
        return false;
    }
    let mut seen = vec![false; task_count];
    for &task in order {
        let Some(slot) = seen.get_mut(task) else {
            return false;
        };
        if *slot {
            return false;
        }
        *slot = true;
    }
    true
}

fn consume_crash(point: CrashPoint, requested: &[[bool; 3]], consumed: &mut [[bool; 3]]) -> bool {
    let Ok(index) = usize::try_from(point.ordinal().saturating_sub(1)) else {
        return false;
    };
    let slot = point.slot();
    if requested.get(index).is_some_and(|stages| stages[slot])
        && consumed.get(index).is_some_and(|stages| !stages[slot])
    {
        consumed[index][slot] = true;
        true
    } else {
        false
    }
}

fn protocol_phase(terminal: TerminalPhase) -> ProtocolPhase {
    match terminal {
        TerminalPhase::Ready => ProtocolPhase::Ready,
        TerminalPhase::Completed => ProtocolPhase::Completed,
        TerminalPhase::Failed => ProtocolPhase::Failed,
        TerminalPhase::Incomplete => ProtocolPhase::Incomplete,
        TerminalPhase::Cancelled => ProtocolPhase::Cancelled,
        TerminalPhase::ResourceLimited => ProtocolPhase::ResourceLimited,
        TerminalPhase::Rejected => ProtocolPhase::Rejected,
    }
}

fn protocol_error<K, S, F, I>(
    reason: DurableError,
    journal: DurableJournal<K, S, F, I>,
    receipts: PublicationLedger<K>,
    started_tasks: usize,
) -> ProtocolError<K, S, F, I>
where
    K: Clone,
{
    let observation = match Arc::try_unwrap(receipts.event_ids) {
        Ok(event_ids) => event_ids,
        Err(shared) => shared.as_ref().clone(),
    };
    ProtocolError {
        reason,
        journal,
        observation,
        started_tasks,
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_report<K, P, S, F, I>(
    plan: &PlanIdentity<K, P>,
    phase: TerminalPhase,
    journal: DurableJournal<K, S, F, I>,
    receipts: PublicationLedger<K>,
    drafts: Vec<SnapshotDraft>,
    steps: u64,
    input_cells: u64,
) -> ProtocolReport<K, S, F, I>
where
    K: Clone + Ord,
    P: AsRef<str> + Clone + Eq,
    S: Clone + Eq,
    F: Clone + Eq,
    I: Clone + Eq,
{
    let structural_plan = StructuralPlanIdentity {
        signature: plan.signature.clone(),
    };
    let event_storage = journal.events.clone();
    let receipt_storage = receipts.event_ids.clone();
    let mut snapshots = Vec::with_capacity(drafts.len());
    for draft in drafts {
        snapshots.push(ProtocolSnapshot {
            plan: structural_plan.clone(),
            phase: draft.phase,
            cursor: draft.cursor,
            journal: JournalSnapshot {
                plan: structural_plan.signature.clone(),
                events: event_storage.clone(),
                len: draft.journal_len,
            },
            receipts: PublicationSnapshot {
                event_ids: receipt_storage.clone(),
                len: draft.receipt_len,
            },
            remaining_replay_events: 0,
            point: draft.point,
        });
    }
    ProtocolReport {
        plan: structural_plan,
        phase,
        journal,
        receipts,
        snapshots,
        work: DurableWorkProfile {
            steps,
            input_cells,
            heap_cells: input_cells.saturating_add(1).saturating_mul(7),
            maximum_native_frames: 1,
        },
    }
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        Checkpoint, CrashPoint, DurableError, DurableJournal, DurableOutcome, ExternalKeyMap,
        PlanIdentity, ProtocolInput, ReplayClass, ResumeMachine, TerminalPhase,
    };

    type Outcome = DurableOutcome<u64, u64, u64>;

    fn identity(keys: Vec<u64>) -> PlanIdentity<u64, &'static str> {
        let dependencies = keys.windows(2).map(|pair| (pair[0], pair[1])).collect();
        let effects = keys
            .iter()
            .map(|key| (vec![*key], vec![key.saturating_add(1)]))
            .collect();
        let costs = keys.clone();
        match PlanIdentity::new(
            1,
            keys,
            dependencies,
            effects,
            costs,
            u64::MAX,
            "test-profile",
        ) {
            Ok(identity) => identity,
            Err(error) => panic!("valid generated identity rejected: {error}"),
        }
    }

    fn success_outcomes(count: usize) -> Vec<Outcome> {
        (0..count)
            .map(|value| DurableOutcome::Success(usize_to_u64(value)))
            .collect()
    }

    fn completed(keys: Vec<u64>) -> super::ProtocolReport<u64, u64, u64, u64> {
        let task_count = keys.len();
        match ResumeMachine::run(ProtocolInput::new(
            identity(keys),
            success_outcomes(task_count),
        )) {
            Ok(report) => report,
            Err(error) => panic!("valid generated run rejected: {}", error.reason()),
        }
    }

    #[test]
    fn arbitrary_input_order_canonicalizes_exact_identity() {
        let shuffled = PlanIdentity::new(
            1,
            vec![30, 10, 20],
            vec![(20, 30), (10, 20)],
            vec![
                (vec![30], vec![31]),
                (vec![10], vec![11]),
                (vec![20], vec![21]),
            ],
            vec![3, 1, 2],
            9,
            "test-profile",
        );
        let canonical = PlanIdentity::new(
            1,
            vec![10, 20, 30],
            vec![(10, 20), (20, 30)],
            vec![
                (vec![10], vec![11]),
                (vec![20], vec![21]),
                (vec![30], vec![31]),
            ],
            vec![1, 2, 3],
            9,
            "test-profile",
        );
        assert_eq!(shuffled, canonical);
    }

    #[test]
    fn duplicate_keys_fail_before_dense_mapping() {
        assert_eq!(
            ExternalKeyMap::new([2, 1, 2]),
            Err(DurableError::DuplicateExternalKey)
        );
    }

    #[test]
    fn boundary_append_enforces_terminal_language() {
        let plan = identity(vec![10]);
        let journal: DurableJournal<u64, u64, u64, u64> = DurableJournal::new(plan);
        assert_eq!(
            journal.clone().append_terminal(DurableOutcome::Completed),
            Err(DurableError::InvalidEvent)
        );
        assert!(journal.append_terminal(DurableOutcome::Cancelled).is_ok());
    }

    #[test]
    fn invalid_replay_key_and_crash_ordinal_fail_before_effects() {
        let unknown_key = ProtocolInput::new(identity(vec![10]), success_outcomes(1))
            .with_replay_class(11, ReplayClass::Unsafe);
        let Err(error) = ResumeMachine::run(unknown_key) else {
            panic!("unknown replay key was accepted");
        };
        assert_eq!(error.reason(), DurableError::InvalidProtocolConfiguration);
        assert_eq!(error.started_tasks(), 0);

        let invalid_crash = ProtocolInput::new(identity(vec![10]), success_outcomes(1))
            .with_crashes([CrashPoint::BeforeJournal { ordinal: 0 }]);
        let Err(error) = ResumeMachine::run(invalid_crash) else {
            panic!("zero crash ordinal was accepted");
        };
        assert_eq!(error.reason(), DurableError::InvalidProtocolConfiguration);
        assert_eq!(error.started_tasks(), 0);
    }

    #[test]
    fn checkpoint_payload_variant_mismatch_fails_before_effects() {
        let baseline = completed(vec![10, 20]);
        let checkpoint = match Checkpoint::from_committed_prefix(&baseline, 1) {
            Ok(checkpoint) => checkpoint,
            Err(error) => panic!("valid prefix rejected: {error}"),
        };
        let input = ProtocolInput::resume(
            identity(vec![10, 20]),
            vec![DurableOutcome::Failure(7), DurableOutcome::Success(2)],
            checkpoint,
        );
        let Err(error) = ResumeMachine::run(input) else {
            panic!("checkpoint payload-kind mismatch was accepted");
        };
        assert_eq!(error.reason(), DurableError::MalformedCheckpoint);
        assert_eq!(error.started_tasks(), 0);
    }

    #[test]
    fn cancellation_wins_a_simultaneous_resource_boundary() {
        let input = ProtocolInput::new(identity(vec![10]), success_outcomes(1))
            .with_cancellation_after(0)
            .with_resource_limit_after(0);
        let report = match ResumeMachine::run(input) {
            Ok(report) => report,
            Err(error) => panic!("valid boundary rejected: {}", error.reason()),
        };
        assert_eq!(report.phase(), TerminalPhase::Cancelled);
    }

    #[test]
    fn iterative_permutations_are_complete_and_unique() {
        let permutations: BTreeSet<_> = ProtocolInput::completion_permutations(4).collect();
        assert_eq!(permutations.len(), 24);
        assert!(permutations
            .iter()
            .all(|permutation| permutation.len() == 4));
    }

    #[test]
    fn deterministic_shuffles_have_serial_observation() {
        const TASKS: usize = 32;
        let serial = completed((0..TASKS).map(usize_to_u64).collect());
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..256 {
            let mut order: Vec<_> = (0..TASKS).collect();
            for upper in (1..TASKS).rev() {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                let index = usize::try_from(state % (usize_to_u64(upper) + 1)).unwrap_or(0);
                order.swap(upper, index);
            }
            let input = ProtocolInput::new(
                identity((0..TASKS).map(usize_to_u64).collect()),
                success_outcomes(TASKS),
            )
            .with_completion_order(order);
            let parallel = match ResumeMachine::run(input) {
                Ok(report) => report,
                Err(error) => panic!("valid permutation rejected: {}", error.reason()),
            };
            assert_eq!(parallel.observation(), serial.observation());
        }
    }

    fn usize_to_u64(value: usize) -> u64 {
        u64::try_from(value).unwrap_or(u64::MAX)
    }
}
