# Rust API

## Contract

A task has a stable identifier, caller payload, canonical read/write effects,
and positive cost. A dependency pair `(source, target)` means the source must
commit from a strictly earlier batch. `PlanBuilder::build` either returns one
immutable canonical plan or rejects atomically.

`TaskExecutor` is the one-task synchronous interface. `SerialExecutor` lifts it
to `BatchExecutor`, the runtime-neutral complete-batch interface. A parallel
adapter may return current-batch completions in any order; schedlib validates
exact membership and completeness before calling `CommitSink` in stable order.

## Complete example

```rust
use schedlib::{
    Budget, CommitSink, CommittedTask, Cost, EffectSet, NeverCancel, PlanBuilder,
    SerialExecutor, TaskEffects, TaskExecution, TaskExecutor, TaskId, TaskSpec,
    TaskView,
};

struct LengthWorker;

impl TaskExecutor<&'static str> for LengthWorker {
    type Output = usize;
    type Error = &'static str;
    type Incomplete = &'static str;

    fn execute(
        &mut self,
        task: TaskView<'_, &'static str>,
    ) -> TaskExecution<Self::Output, Self::Error, Self::Incomplete> {
        TaskExecution::Success(task.payload().len())
    }
}

#[derive(Default)]
struct Published(Vec<TaskId>);

impl CommitSink<usize, &'static str, &'static str> for Published {
    fn commit(
        &mut self,
        result: &CommittedTask<usize, &'static str, &'static str>,
    ) {
        self.0.push(result.task_id());
    }
}

fn main() {
    let no_effects = || {
        TaskEffects::new(
            EffectSet::from_resources(std::iter::empty()),
            EffectSet::from_resources(std::iter::empty()),
        )
    };
    let tasks = vec![
        TaskSpec::new(
            TaskId::new(10),
            "parse",
            no_effects(),
            Cost::new(1).expect("one is positive"),
        ),
        TaskSpec::new(
            TaskId::new(20),
            "optimize",
            no_effects(),
            Cost::new(1).expect("one is positive"),
        ),
    ];
    let plan = PlanBuilder::new(Budget::new(2).expect("two is positive"))
        .tasks(tasks)
        .dependencies([(TaskId::new(10), TaskId::new(20))])
        .build()
        .expect("the dependency is acyclic and both tasks fit the budget");

    let mut executor = SerialExecutor::new(LengthWorker);
    let mut cancellation = NeverCancel;
    let mut published = Published::default();
    let report = plan
        .execute(&mut executor, &mut cancellation, &mut published)
        .expect("the serial executor returns exactly one completion per task");

    assert_eq!(published.0, vec![TaskId::new(10), TaskId::new(20)]);
    assert_eq!(report.committed().len(), 2);
}
```

The same program is the compile-checked `examples/serial.rs` target. Run it
with `cargo run --example serial`.

## Errors and terminal outcomes

`PlanError::Cyclic` takes precedence when an input is both cyclic and over
budget, matching the formal validation transition. `ResourceExhausted` names
the first stable task whose individual cost exceeds the budget. Duplicate task
identifiers and unknown dependency endpoints are rejected before publication.

`TaskExecution::Failure` and `TaskExecution::Incomplete` are distinct terminal
outcomes. The first non-success in canonical order is committed and terminates
the report; later completed results remain unobservable. Cancellation is
acknowledged before the next commit at an exact prefix length. Empty plans
complete with zero worker or sink calls.

## Parallel adapters

A parallel adapter implements `BatchExecutor` and receives a `BatchView` whose
members are pairwise independent and within budget. It must return exactly one
`TaskCompletion` for every current member. Foreign, future, duplicate, or
missing completions cause `ExecutionError` before the current batch publishes.
The adapter owns runtime-specific worker limits, panic handling, and shutdown;
it cannot alter plan or commit order.

### Built-in Rayon adapter

Enable the optional feature and run the compile-checked example:

```sh
cargo run --release --features rayon --example rayon
```

Implement `ParallelTaskExecutor<T>` with an immutable `&self` callback, choose
an exact positive worker count, and construct one reusable owned pool:

```rust
use std::num::NonZeroUsize;
use schedlib::{RayonBuildError, RayonConfig, RayonExecutor};

fn build_executor<W>(worker: W) -> Result<RayonExecutor<W>, RayonBuildError> {
    let threads = NonZeroUsize::new(4).expect("four is positive");
    let config = RayonConfig::new(threads);
    RayonExecutor::new(worker, config)
}
```

The full, semantically checked program is
[`examples/rayon.rs`](../../examples/rayon.rs). A caller may add
`with_worker_stack_size` when it needs an explicit native-stack budget.
Task payloads must be `Sync`, and output, failure, and incomplete values must be
`Send`. The adapter returns only after all current-batch callbacks join. Worker
panics propagate according to Rayon and are never relabeled as typed outcomes.
