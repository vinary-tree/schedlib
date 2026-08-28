use core::fmt;

use crate::{Cost, Plan, PlanBatch, TaskEffects, TaskId, TaskSpec};

type ExecutorReport<E, T> = ExecutionReport<
    <E as BatchExecutor<T>>::Output,
    <E as BatchExecutor<T>>::Error,
    <E as BatchExecutor<T>>::Incomplete,
>;
type CompletionSlot<E, T> = Option<
    TaskExecution<
        <E as BatchExecutor<T>>::Output,
        <E as BatchExecutor<T>>::Error,
        <E as BatchExecutor<T>>::Incomplete,
    >,
>;

/// Worker result for one task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskExecution<O, E, I> {
    /// Task completed successfully with an output.
    Success(O),
    /// Task completed with a terminal failure.
    Failure(E),
    /// Task could not complete and preserves a structured reason.
    Incomplete(I),
}

/// One worker completion paired with its stable task identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCompletion<O, E, I> {
    task_id: TaskId,
    outcome: TaskExecution<O, E, I>,
}

impl<O, E, I> TaskCompletion<O, E, I> {
    /// Creates a completion reported by a batch executor.
    #[must_use]
    pub const fn new(task_id: TaskId, outcome: TaskExecution<O, E, I>) -> Self {
        Self { task_id, outcome }
    }

    /// Returns the completed task identifier.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    /// Returns the worker outcome.
    #[must_use]
    pub const fn outcome(&self) -> &TaskExecution<O, E, I> {
        &self.outcome
    }
}

/// Borrowed immutable view of one task supplied to a serial worker.
#[derive(Debug, Clone, Copy)]
pub struct TaskView<'a, T> {
    task: &'a TaskSpec<T>,
}

impl<'a, T> TaskView<'a, T> {
    /// Returns the stable task identifier.
    #[must_use]
    pub const fn id(&self) -> TaskId {
        self.task.id()
    }

    /// Returns the caller payload.
    #[must_use]
    pub const fn payload(&self) -> &'a T {
        self.task.payload()
    }

    /// Returns canonical effect metadata.
    #[must_use]
    pub const fn effects(&self) -> &'a TaskEffects {
        self.task.effects()
    }

    /// Returns the positive task cost.
    #[must_use]
    pub const fn cost(&self) -> Cost {
        self.task.cost()
    }
}

/// Borrowed immutable view of one independent plan batch.
#[derive(Debug, Clone, Copy)]
pub struct BatchView<'a, T> {
    plan: &'a Plan<T>,
    batch: &'a PlanBatch,
    batch_index: usize,
}

impl<'a, T> BatchView<'a, T> {
    /// Returns batch members in canonical commit order.
    #[must_use]
    pub fn task_ids(&self) -> &'a [TaskId] {
        self.batch.task_ids()
    }

    /// Returns the batch's total resource cost.
    #[must_use]
    pub const fn total_cost(&self) -> Cost {
        self.batch.total_cost()
    }

    /// Returns an immutable task view for a member identifier.
    #[must_use]
    pub fn task(&self, task_id: TaskId) -> Option<TaskView<'a, T>> {
        if self
            .plan
            .task_location(task_id)
            .is_none_or(|(batch_index, _)| batch_index != self.batch_index)
        {
            return None;
        }
        self.plan.task(task_id).map(|task| TaskView { task })
    }
}

/// Serial worker callback for one immutable task.
pub trait TaskExecutor<T> {
    /// Successful output type.
    type Output;
    /// Terminal failure type.
    type Error;
    /// Structured incomplete reason type.
    type Incomplete;

    /// Executes one task synchronously.
    fn execute(
        &mut self,
        task: TaskView<'_, T>,
    ) -> TaskExecution<Self::Output, Self::Error, Self::Incomplete>;
}

/// Runtime-neutral executor for one complete independent batch.
///
/// Implementations may return completions in any order. schedlib validates
/// exact batch coverage before publishing any result, then commits in stable
/// batch order.
pub trait BatchExecutor<T> {
    /// Successful output type.
    type Output;
    /// Terminal failure type.
    type Error;
    /// Structured incomplete reason type.
    type Incomplete;

    /// Executes every task in one batch and returns all completions.
    fn execute_batch(
        &mut self,
        batch: BatchView<'_, T>,
    ) -> Vec<TaskCompletion<Self::Output, Self::Error, Self::Incomplete>>;
}

/// Synchronous serial reference adapter over a one-task executor.
#[derive(Debug, Clone)]
pub struct SerialExecutor<W> {
    inner: W,
}

impl<W> SerialExecutor<W> {
    /// Creates a serial reference adapter.
    #[must_use]
    pub const fn new(inner: W) -> Self {
        Self { inner }
    }

    /// Returns the injected task executor.
    #[must_use]
    pub const fn inner(&self) -> &W {
        &self.inner
    }

    /// Returns the injected task executor mutably.
    #[must_use]
    pub const fn inner_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    /// Consumes the adapter and returns its task executor.
    #[must_use]
    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<T, W> BatchExecutor<T> for SerialExecutor<W>
where
    W: TaskExecutor<T>,
{
    type Output = W::Output;
    type Error = W::Error;
    type Incomplete = W::Incomplete;

    fn execute_batch(
        &mut self,
        batch: BatchView<'_, T>,
    ) -> Vec<TaskCompletion<Self::Output, Self::Error, Self::Incomplete>> {
        let mut completions = Vec::with_capacity(batch.task_ids().len());
        for task_id in batch.task_ids() {
            if let Some(task) = batch.task(*task_id) {
                completions.push(TaskCompletion::new(*task_id, self.inner.execute(task)));
            }
        }
        completions
    }
}

/// Monotone cancellation observation at a canonical commit boundary.
pub trait Cancellation {
    /// Returns whether cancellation is requested at the supplied committed
    /// prefix length. Once true, an implementation must remain true.
    fn requested(&self, committed: usize) -> bool;
}

/// Cancellation source that never requests cancellation.
#[derive(Debug, Clone, Copy, Default)]
pub struct NeverCancel;

impl Cancellation for NeverCancel {
    fn requested(&self, _committed: usize) -> bool {
        false
    }
}

/// Deterministic cancellation source activated at one commit-prefix length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancelAfter {
    committed: usize,
}

impl CancelAfter {
    /// Creates a cancellation source for an exact committed-prefix boundary.
    #[must_use]
    pub const fn new(committed: usize) -> Self {
        Self { committed }
    }

    /// Returns the configured committed-prefix boundary.
    #[must_use]
    pub const fn boundary(self) -> usize {
        self.committed
    }
}

impl Cancellation for CancelAfter {
    fn requested(&self, committed: usize) -> bool {
        committed >= self.committed
    }
}

/// One canonical committed observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedTask<O, E, I> {
    task_id: TaskId,
    outcome: TaskExecution<O, E, I>,
}

impl<O, E, I> CommittedTask<O, E, I> {
    /// Returns the stable task identifier.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    /// Returns the canonical worker outcome.
    #[must_use]
    pub const fn outcome(&self) -> &TaskExecution<O, E, I> {
        &self.outcome
    }
}

/// Ordered publication callback.
pub trait CommitSink<O, E, I> {
    /// Observes exactly one next canonical result.
    fn commit(&mut self, committed: &CommittedTask<O, E, I>);
}

/// Terminal execution phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionPhase {
    /// Every task committed successfully.
    Completed,
    /// The first canonical failure committed and terminated execution.
    Failed,
    /// The first canonical incomplete result committed and terminated
    /// execution.
    Incomplete,
    /// Cancellation was acknowledged at an exact commit boundary.
    Cancelled,
}

/// Exact logical work for one accepted execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExecutionWorkProfile {
    batches_dispatched: u64,
    tasks_dispatched: u64,
    completions_validated: u64,
    commits: u64,
    result_slots_peak: u64,
    allocation_count: u64,
}

impl ExecutionWorkProfile {
    /// Returns the number of complete batches dispatched.
    #[must_use]
    pub const fn batches_dispatched(self) -> u64 {
        self.batches_dispatched
    }

    /// Returns the number of tasks dispatched.
    #[must_use]
    pub const fn tasks_dispatched(self) -> u64 {
        self.tasks_dispatched
    }

    /// Returns the number of completion records validated.
    #[must_use]
    pub const fn completions_validated(self) -> u64 {
        self.completions_validated
    }

    /// Returns the number of canonical commits.
    #[must_use]
    pub const fn commits(self) -> u64 {
        self.commits
    }

    /// Returns the peak current-batch result-slot count.
    #[must_use]
    pub const fn result_slots_peak(self) -> u64 {
        self.result_slots_peak
    }

    /// Returns scheduler-owned execution allocation events.
    #[must_use]
    pub const fn allocation_count(self) -> u64 {
        self.allocation_count
    }

    /// Returns total scheduler logical work after planning.
    #[must_use]
    pub const fn logical_work(self) -> u64 {
        self.batches_dispatched
            .saturating_add(self.tasks_dispatched)
            .saturating_add(self.completions_validated)
            .saturating_add(self.commits)
    }
}

/// Terminal report containing the exact canonical observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReport<O, E, I> {
    phase: ExecutionPhase,
    committed: Vec<CommittedTask<O, E, I>>,
    work_profile: ExecutionWorkProfile,
}

impl<O, E, I> ExecutionReport<O, E, I> {
    /// Returns the terminal phase.
    #[must_use]
    pub const fn phase(&self) -> ExecutionPhase {
        self.phase
    }

    /// Returns the committed canonical prefix.
    #[must_use]
    pub fn committed(&self) -> &[CommittedTask<O, E, I>] {
        &self.committed
    }

    /// Returns exact execution work evidence.
    #[must_use]
    pub const fn work_profile(&self) -> ExecutionWorkProfile {
        self.work_profile
    }
}

/// Batch-executor contract violation detected before publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    /// A completion does not belong to the current batch.
    ForeignCompletion {
        /// Invalid completion task.
        task: TaskId,
    },
    /// A current-batch task completed more than once.
    DuplicateCompletion {
        /// Duplicated task.
        task: TaskId,
    },
    /// A current-batch task did not produce a completion.
    MissingCompletion {
        /// Missing task.
        task: TaskId,
    },
    /// A checked internal representation invariant failed.
    InternalInvariant(&'static str),
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignCompletion { task } => {
                write!(
                    formatter,
                    "completion for task {task} is outside the current batch"
                )
            }
            Self::DuplicateCompletion { task } => {
                write!(formatter, "task {task} completed more than once")
            }
            Self::MissingCompletion { task } => {
                write!(formatter, "task {task} did not complete in its batch")
            }
            Self::InternalInvariant(reason) => {
                write!(
                    formatter,
                    "execution representation invariant failed: {reason}"
                )
            }
        }
    }
}

impl std::error::Error for ExecutionError {}

impl<T> Plan<T> {
    /// Executes an immutable plan through an injected batch executor and
    /// canonical ordered-commit sink.
    ///
    /// # Errors
    ///
    /// Returns an error before publishing the current batch when the executor
    /// reports a foreign, duplicate, or missing completion.
    pub fn execute<E, C, S>(
        &self,
        executor: &mut E,
        cancellation: &mut C,
        sink: &mut S,
    ) -> Result<ExecutorReport<E, T>, ExecutionError>
    where
        E: BatchExecutor<T>,
        C: Cancellation,
        S: CommitSink<E::Output, E::Error, E::Incomplete>,
    {
        let mut committed = Vec::with_capacity(self.task_count());
        let mut work = ExecutionWorkProfile {
            allocation_count: u64::from(!self.is_empty()),
            ..ExecutionWorkProfile::default()
        };
        if self.is_empty() {
            return Ok(ExecutionReport {
                phase: ExecutionPhase::Completed,
                committed,
                work_profile: work,
            });
        }

        for (batch_index, batch) in self.batches().iter().enumerate() {
            if cancellation.requested(committed.len()) {
                return Ok(ExecutionReport {
                    phase: ExecutionPhase::Cancelled,
                    committed,
                    work_profile: work,
                });
            }
            let view = BatchView {
                plan: self,
                batch,
                batch_index,
            };
            let completions = executor.execute_batch(view);
            let mut result_slots: Vec<CompletionSlot<E, T>> = core::iter::repeat_with(|| None)
                .take(batch.task_ids().len())
                .collect();
            work.allocation_count += u64::from(!result_slots.is_empty());
            work.result_slots_peak = work.result_slots_peak.max(result_slots.len() as u64);
            for completion in completions {
                let task_id = completion.task_id;
                let (completion_batch, index) = self
                    .task_location(task_id)
                    .ok_or(ExecutionError::ForeignCompletion { task: task_id })?;
                if completion_batch != batch_index {
                    return Err(ExecutionError::ForeignCompletion { task: task_id });
                }
                let slot = result_slots
                    .get_mut(index)
                    .ok_or(ExecutionError::InternalInvariant(
                        "completion index lies outside result slots",
                    ))?;
                if slot.is_some() {
                    return Err(ExecutionError::DuplicateCompletion { task: task_id });
                }
                *slot = Some(completion.outcome);
                work.completions_validated += 1;
            }
            for (index, slot) in result_slots.iter().enumerate() {
                if slot.is_none() {
                    let task = batch.task_ids().get(index).copied().ok_or(
                        ExecutionError::InternalInvariant("result slot lies outside current batch"),
                    )?;
                    return Err(ExecutionError::MissingCompletion { task });
                }
            }
            work.batches_dispatched += 1;
            work.tasks_dispatched += batch.task_ids().len() as u64;

            for (index, slot) in result_slots.iter_mut().enumerate() {
                if cancellation.requested(committed.len()) {
                    return Ok(ExecutionReport {
                        phase: ExecutionPhase::Cancelled,
                        committed,
                        work_profile: work,
                    });
                }
                let task_id = batch.task_ids().get(index).copied().ok_or(
                    ExecutionError::InternalInvariant("commit index lies outside current batch"),
                )?;
                let outcome = slot.take().ok_or(ExecutionError::InternalInvariant(
                    "validated result slot is unexpectedly empty",
                ))?;
                let terminal = match &outcome {
                    TaskExecution::Success(_) => None,
                    TaskExecution::Failure(_) => Some(ExecutionPhase::Failed),
                    TaskExecution::Incomplete(_) => Some(ExecutionPhase::Incomplete),
                };
                let observation = CommittedTask { task_id, outcome };
                sink.commit(&observation);
                committed.push(observation);
                work.commits += 1;
                if let Some(phase) = terminal {
                    return Ok(ExecutionReport {
                        phase,
                        committed,
                        work_profile: work,
                    });
                }
            }
        }

        Ok(ExecutionReport {
            phase: ExecutionPhase::Completed,
            committed,
            work_profile: work,
        })
    }
}
