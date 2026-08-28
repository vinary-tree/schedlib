#![cfg(feature = "rayon")]

use std::collections::{HashSet, VecDeque};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Condvar, Mutex};
use std::thread;

use schedlib::{
    Budget, CommitSink, CommittedTask, ControlModel, Cost, EffectSet, ExecutionPhase,
    ExecutionReport, NeverCancel, ParallelTaskExecutor, Plan, PlanBuilder, RayonConfig,
    RayonExecutor, SerialExecutor, TaskEffects, TaskExecution, TaskExecutor, TaskId, TaskSpec,
    TaskView,
};

type TestReport = ExecutionReport<u64, (), ()>;
type CanonicalObservation = (ExecutionPhase, Vec<CommittedTask<u64, (), ()>>, Vec<TaskId>);

fn positive_usize(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("the test fixture uses a positive value")
}

fn empty_effects() -> TaskEffects {
    TaskEffects::new(
        EffectSet::from_resources(std::iter::empty()),
        EffectSet::from_resources(std::iter::empty()),
    )
}

fn independent_plan(task_count: usize) -> Plan<u32> {
    let tasks = (0..task_count)
        .map(|index| {
            let id = u32::try_from(index).expect("test task identifiers fit u32");
            TaskSpec::new(
                TaskId::new(id),
                id,
                empty_effects(),
                Cost::new(1).expect("one is positive"),
            )
        })
        .collect::<Vec<_>>();
    let budget = u64::try_from(task_count.max(1)).expect("test task count fits u64");
    PlanBuilder::new(Budget::new(budget).expect("the test budget is positive"))
        .tasks(tasks)
        .dependencies(std::iter::empty())
        .build()
        .expect("independent test tasks form one accepted plan")
}

#[derive(Default)]
struct RecordingSink(Vec<TaskId>);

impl CommitSink<u64, (), ()> for RecordingSink {
    fn commit(&mut self, committed: &CommittedTask<u64, (), ()>) {
        self.0.push(committed.task_id());
    }
}

#[derive(Clone)]
struct Observations {
    calls: Arc<Vec<AtomicUsize>>,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    finished: Arc<AtomicUsize>,
    physical_order: Arc<Mutex<Vec<TaskId>>>,
}

impl Observations {
    fn new(task_count: usize) -> Self {
        Self {
            calls: Arc::new((0..task_count).map(|_| AtomicUsize::new(0)).collect()),
            active: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
            finished: Arc::new(AtomicUsize::new(0)),
            physical_order: Arc::new(Mutex::new(Vec::with_capacity(task_count))),
        }
    }
}

#[derive(Clone)]
struct ObservedWorker {
    observations: Observations,
    timing_seed: u64,
}

impl ParallelTaskExecutor<u32> for ObservedWorker {
    type Output = u64;
    type Error = ();
    type Incomplete = ();

    fn execute(
        &self,
        task: TaskView<'_, u32>,
    ) -> TaskExecution<Self::Output, Self::Error, Self::Incomplete> {
        let active = self.observations.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.observations.peak.fetch_max(active, Ordering::SeqCst);
        let task_index = task.id().index();
        self.observations.calls[task_index].fetch_add(1, Ordering::SeqCst);
        let rounds = (self.timing_seed ^ u64::from(task.id().get()).wrapping_mul(0x9e37_79b9)) & 31;
        for round in 0..rounds {
            std::hint::black_box(round);
            if round % 4 == 0 {
                thread::yield_now();
            }
        }
        self.observations
            .physical_order
            .lock()
            .expect("test observation lock is not poisoned")
            .push(task.id());
        self.observations.finished.fetch_add(1, Ordering::SeqCst);
        self.observations.active.fetch_sub(1, Ordering::SeqCst);
        TaskExecution::Success(u64::from(*task.payload()).saturating_mul(17))
    }
}

struct SerialWorker;

impl TaskExecutor<u32> for SerialWorker {
    type Output = u64;
    type Error = ();
    type Incomplete = ();

    fn execute(
        &mut self,
        task: TaskView<'_, u32>,
    ) -> TaskExecution<Self::Output, Self::Error, Self::Incomplete> {
        TaskExecution::Success(u64::from(*task.payload()).saturating_mul(17))
    }
}

fn execute_parallel(
    plan: &Plan<u32>,
    worker_threads: usize,
    worker: ObservedWorker,
) -> (TestReport, RecordingSink) {
    let config = RayonConfig::new(positive_usize(worker_threads));
    let mut executor =
        RayonExecutor::new(worker, config).expect("the local Rayon pool must be constructible");
    let mut cancellation = NeverCancel;
    let mut sink = RecordingSink::default();
    let report = plan
        .execute(&mut executor, &mut cancellation, &mut sink)
        .expect("the Rayon adapter returns exactly one current-batch completion per task");
    (report, sink)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ReferenceState {
    pending: u8,
    active: u8,
    completed: u8,
    completion_trace: Vec<u8>,
    returned: bool,
}

fn reference_states(worker_limit: u32) -> HashSet<ReferenceState> {
    let initial = ReferenceState {
        pending: 0b1111,
        active: 0,
        completed: 0,
        completion_trace: Vec::new(),
        returned: false,
    };
    let mut discovered = HashSet::from([initial.clone()]);
    let mut queue = VecDeque::from([initial]);
    while let Some(state) = queue.pop_front() {
        let union = state.pending | state.active | state.completed;
        assert_eq!(union, 0b1111);
        assert_eq!(state.pending & state.active, 0);
        assert_eq!(state.pending & state.completed, 0);
        assert_eq!(state.active & state.completed, 0);
        assert!(state.active.count_ones() <= worker_limit);
        assert_eq!(
            state.completion_trace.len(),
            state.completed.count_ones() as usize
        );
        let trace_set = state
            .completion_trace
            .iter()
            .fold(0u8, |set, task| set | (1 << task));
        assert_eq!(trace_set, state.completed);
        if state.returned {
            assert_eq!(state.completed, 0b1111);
            continue;
        }
        if state.active.count_ones() < worker_limit {
            for task in 0..4u8 {
                let bit = 1 << task;
                if state.pending & bit != 0 {
                    let mut next = state.clone();
                    next.pending &= !bit;
                    next.active |= bit;
                    if discovered.insert(next.clone()) {
                        queue.push_back(next);
                    }
                }
            }
        }
        for task in 0..4u8 {
            let bit = 1 << task;
            if state.active & bit != 0 {
                let mut next = state.clone();
                next.active &= !bit;
                next.completed |= bit;
                next.completion_trace.push(task);
                if discovered.insert(next.clone()) {
                    queue.push_back(next);
                }
            }
        }
        if state.pending == 0 && state.active == 0 && state.completed == 0b1111 {
            let mut next = state;
            next.returned = true;
            if discovered.insert(next.clone()) {
                queue.push_back(next);
            }
        }
    }
    discovered
}

fn permutations_of_four() -> Vec<[u32; 4]> {
    let mut values = [0, 1, 2, 3];
    let mut counters = [0usize; 4];
    let mut permutations = vec![values];
    let mut index = 1usize;
    while index < values.len() {
        if counters[index] < index {
            let swap = if index % 2 == 0 { 0 } else { counters[index] };
            values.swap(swap, index);
            permutations.push(values);
            counters[index] += 1;
            index = 1;
        } else {
            counters[index] = 0;
            index += 1;
        }
    }
    permutations
}

struct OrderedController {
    desired: [TaskId; 4],
    state: Mutex<(usize, Vec<TaskId>)>,
    changed: Condvar,
}

struct OrderedWorker(Arc<OrderedController>);

impl ParallelTaskExecutor<u32> for OrderedWorker {
    type Output = u64;
    type Error = ();
    type Incomplete = ();

    fn execute(
        &self,
        task: TaskView<'_, u32>,
    ) -> TaskExecution<Self::Output, Self::Error, Self::Incomplete> {
        let mut state = self
            .0
            .state
            .lock()
            .expect("test completion-order lock is not poisoned");
        while self.0.desired[state.0] != task.id() {
            state = self
                .0
                .changed
                .wait(state)
                .expect("test completion-order lock is not poisoned");
        }
        state.1.push(task.id());
        state.0 += 1;
        self.0.changed.notify_all();
        TaskExecution::Success(u64::from(*task.payload()).saturating_mul(17))
    }
}

struct PeakWorker {
    barrier: Arc<Barrier>,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

impl ParallelTaskExecutor<u32> for PeakWorker {
    type Output = u64;
    type Error = ();
    type Incomplete = ();

    fn execute(
        &self,
        task: TaskView<'_, u32>,
    ) -> TaskExecution<Self::Output, Self::Error, Self::Incomplete> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        self.barrier.wait();
        self.active.fetch_sub(1, Ordering::SeqCst);
        TaskExecution::Success(u64::from(*task.payload()))
    }
}

#[test]
fn exhaustive_rayon_state_shapes() {
    for worker_limit in [1, 2, 4] {
        let states = reference_states(worker_limit);
        assert!(!states.is_empty());
        assert!(states.iter().all(|state| {
            (state.pending | state.active | state.completed) == 0b1111
                && state.active.count_ones() <= worker_limit
        }));
    }
}

#[test]
fn prop_rayon_states_well_typed() {
    let plan = independent_plan(4);
    let observations = Observations::new(4);
    let (report, _) = execute_parallel(
        &plan,
        2,
        ObservedWorker {
            observations,
            timing_seed: 7,
        },
    );
    assert_eq!(report.phase(), ExecutionPhase::Completed);
    assert_eq!(report.committed().len(), 4);
}

#[test]
fn exhaustive_rayon_interleavings() {
    for worker_limit in [1, 2, 4] {
        let returned = reference_states(worker_limit)
            .into_iter()
            .filter(|state| state.returned)
            .map(|state| state.completion_trace)
            .collect::<HashSet<_>>();
        assert_eq!(returned.len(), 24);
    }
}

#[test]
fn prop_rayon_executes_each_task_once() {
    let plan = independent_plan(64);
    let observations = Observations::new(64);
    let worker = ObservedWorker {
        observations: observations.clone(),
        timing_seed: 11,
    };
    let (report, _) = execute_parallel(&plan, 4, worker);
    assert_eq!(report.phase(), ExecutionPhase::Completed);
    assert!(observations
        .calls
        .iter()
        .all(|calls| calls.load(Ordering::SeqCst) == 1));
}

#[test]
fn prop_rayon_worker_limit_is_respected() {
    for worker_threads in [1, 2, 4] {
        let task_count = worker_threads * 2;
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let worker = PeakWorker {
            barrier: Arc::new(Barrier::new(worker_threads)),
            active: active.clone(),
            peak: peak.clone(),
        };
        let plan = independent_plan(task_count);
        let config = RayonConfig::new(positive_usize(worker_threads));
        let mut executor =
            RayonExecutor::new(worker, config).expect("the local Rayon pool must be constructible");
        let mut cancellation = NeverCancel;
        let mut sink = RecordingSink::default();
        let report = plan
            .execute(&mut executor, &mut cancellation, &mut sink)
            .expect("the bounded worker returns a complete current batch");
        assert_eq!(report.phase(), ExecutionPhase::Completed);
        assert_eq!(peak.load(Ordering::SeqCst), worker_threads);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn exhaustive_rayon_worker_counts() {
    let plan = independent_plan(12);
    for worker_threads in 1..=4 {
        let observations = Observations::new(12);
        let (report, sink) = execute_parallel(
            &plan,
            worker_threads,
            ObservedWorker {
                observations,
                timing_seed: 13,
            },
        );
        assert_eq!(report.phase(), ExecutionPhase::Completed);
        assert_eq!(sink.0, plan.flattened_task_ids());
    }
}

struct JoinCheckingSink {
    finished: Arc<AtomicUsize>,
    expected: usize,
}

impl CommitSink<u64, (), ()> for JoinCheckingSink {
    fn commit(&mut self, _committed: &CommittedTask<u64, (), ()>) {
        assert_eq!(self.finished.load(Ordering::SeqCst), self.expected);
    }
}

#[test]
fn prop_rayon_joins_before_commit() {
    let plan = independent_plan(32);
    let observations = Observations::new(32);
    let worker = ObservedWorker {
        observations: observations.clone(),
        timing_seed: 17,
    };
    let mut executor = RayonExecutor::new(worker, RayonConfig::new(positive_usize(4)))
        .expect("the local Rayon pool must be constructible");
    let mut cancellation = NeverCancel;
    let mut sink = JoinCheckingSink {
        finished: observations.finished,
        expected: 32,
    };
    let report = plan
        .execute(&mut executor, &mut cancellation, &mut sink)
        .expect("the Rayon adapter joins before returning the current batch");
    assert_eq!(report.phase(), ExecutionPhase::Completed);
}

#[test]
fn prop_rayon_drop_has_no_detached_workers() {
    let plan = independent_plan(32);
    let observations = Observations::new(32);
    {
        let worker = ObservedWorker {
            observations: observations.clone(),
            timing_seed: 19,
        };
        let (report, _) = execute_parallel(&plan, 4, worker);
        assert_eq!(report.phase(), ExecutionPhase::Completed);
    }
    assert_eq!(observations.active.load(Ordering::SeqCst), 0);
    assert_eq!(observations.finished.load(Ordering::SeqCst), 32);
}

#[test]
fn prop_rayon_matches_serial_across_worker_counts() {
    let plan = independent_plan(32);
    let mut serial = SerialExecutor::new(SerialWorker);
    let mut cancellation = NeverCancel;
    let mut serial_sink = RecordingSink::default();
    let expected = plan
        .execute(&mut serial, &mut cancellation, &mut serial_sink)
        .expect("the serial reference accepts the plan");
    for worker_threads in [1, 2, 4] {
        let observations = Observations::new(32);
        let (actual, sink) = execute_parallel(
            &plan,
            worker_threads,
            ObservedWorker {
                observations,
                timing_seed: 23,
            },
        );
        assert_eq!(actual.phase(), expected.phase());
        assert_eq!(actual.committed(), expected.committed());
        assert_eq!(sink.0, serial_sink.0);
    }
}

#[test]
fn prop_rayon_commit_provenance_is_canonical() {
    let plan = independent_plan(64);
    let observations = Observations::new(64);
    let (_, sink) = execute_parallel(
        &plan,
        4,
        ObservedWorker {
            observations,
            timing_seed: 29,
        },
    );
    assert_eq!(sink.0, plan.flattened_task_ids());
}

#[test]
fn prop_rayon_timing_is_unobservable() {
    let plan = independent_plan(64);
    let mut expected: Option<CanonicalObservation> = None;
    for worker_threads in [1, 2, 4] {
        for timing_seed in 0..8 {
            let observations = Observations::new(64);
            let (report, sink) = execute_parallel(
                &plan,
                worker_threads,
                ObservedWorker {
                    observations,
                    timing_seed,
                },
            );
            let observation = (report.phase(), report.committed().to_vec(), sink.0);
            if let Some(reference) = &expected {
                assert_eq!(&observation, reference);
            } else {
                expected = Some(observation);
            }
        }
    }
}

#[test]
fn exhaustive_rayon_completion_permutations() {
    let plan = independent_plan(4);
    for permutation in permutations_of_four() {
        let controller = Arc::new(OrderedController {
            desired: permutation.map(TaskId::new),
            state: Mutex::new((0, Vec::with_capacity(4))),
            changed: Condvar::new(),
        });
        let mut executor = RayonExecutor::new(
            OrderedWorker(controller.clone()),
            RayonConfig::new(positive_usize(4)),
        )
        .expect("the local Rayon pool must be constructible");
        let mut cancellation = NeverCancel;
        let mut sink = RecordingSink::default();
        let report = plan
            .execute(&mut executor, &mut cancellation, &mut sink)
            .expect("the forced completion permutation remains a complete batch");
        let physical = controller
            .state
            .lock()
            .expect("test completion-order lock is not poisoned")
            .1
            .clone();
        assert_eq!(physical, controller.desired);
        assert_eq!(report.phase(), ExecutionPhase::Completed);
        assert_eq!(sink.0, plan.flattened_task_ids());
    }
}

#[test]
fn prop_rayon_finite_batch_terminates() {
    let plan = independent_plan(20_000);
    let observations = Observations::new(20_000);
    let (report, _) = execute_parallel(
        &plan,
        4,
        ObservedWorker {
            observations,
            timing_seed: 31,
        },
    );
    assert_eq!(report.phase(), ExecutionPhase::Completed);
    assert_eq!(report.committed().len(), 20_000);
}

#[test]
fn prop_rayon_empty_plan_bypasses_pool() {
    let plan = independent_plan(0);
    let observations = Observations::new(0);
    let worker = ObservedWorker {
        observations: observations.clone(),
        timing_seed: 37,
    };
    let (report, sink) = execute_parallel(&plan, 2, worker);
    assert_eq!(report.phase(), ExecutionPhase::Completed);
    assert!(report.committed().is_empty());
    assert!(sink.0.is_empty());
    assert_eq!(observations.finished.load(Ordering::SeqCst), 0);
}

#[test]
fn small_stack_large_parallel_batch() {
    thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let task_count = 20_000;
            let plan = independent_plan(task_count);
            let observations = Observations::new(task_count);
            let config = RayonConfig::new(positive_usize(4))
                .with_worker_stack_size(positive_usize(64 * 1024));
            let worker = ObservedWorker {
                observations,
                timing_seed: 41,
            };
            let mut executor = RayonExecutor::new(worker, config)
                .expect("the small-stack local Rayon pool must be constructible");
            let mut cancellation = NeverCancel;
            let mut sink = RecordingSink::default();
            let report = plan
                .execute(&mut executor, &mut cancellation, &mut sink)
                .expect("large independent batch remains stack-safe");
            assert_eq!(report.phase(), ExecutionPhase::Completed);
            assert_eq!(report.committed().len(), task_count);
        })
        .expect("small-stack caller thread must be constructible")
        .join()
        .expect("small-stack parallel execution must not overflow or panic");
}

#[test]
fn prop_no_scheduler_recursive_parallel_control() {
    let observations = Observations::new(1);
    let executor = RayonExecutor::new(
        ObservedWorker {
            observations,
            timing_seed: 43,
        },
        RayonConfig::new(positive_usize(2)),
    )
    .expect("the local Rayon pool must be constructible");
    assert_eq!(executor.control_model(), ControlModel::ParallelIndexedJoin);
}
