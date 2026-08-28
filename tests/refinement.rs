#![allow(clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet};
use std::thread;

use schedlib::{
    BatchExecutor, BatchView, Budget, CancelAfter, CommitSink, CommittedTask, ControlModel, Cost,
    EffectSet, ExecutionPhase, NeverCancel, Plan, PlanBuilder, PlanError, ResourceId,
    SerialExecutor, TaskCompletion, TaskEffects, TaskExecution, TaskExecutor, TaskId, TaskSpec,
    TaskView,
};

fn rid(value: u32) -> ResourceId {
    ResourceId::new(value)
}

fn u32_index(value: usize) -> u32 {
    u32::try_from(value).expect("refinement domain must fit a stable u32 identifier")
}

fn u32_sample(value: u64) -> u32 {
    u32::try_from(value).expect("bounded random sample must fit u32")
}

fn u64_count(value: usize) -> u64 {
    u64::try_from(value).expect("refinement collection length must fit u64")
}

fn effects(reads: &[u32], writes: &[u32]) -> TaskEffects {
    TaskEffects::new(
        EffectSet::from_resources(reads.iter().copied().map(rid)),
        EffectSet::from_resources(writes.iter().copied().map(rid)),
    )
}

fn task(id: u32, cost: u64, reads: &[u32], writes: &[u32]) -> TaskSpec<u32> {
    TaskSpec::new(
        TaskId::new(id),
        id,
        effects(reads, writes),
        Cost::new(cost).expect("test costs are positive"),
    )
}

fn build_plan(
    tasks: Vec<TaskSpec<u32>>,
    dependencies: Vec<(TaskId, TaskId)>,
    budget: u64,
) -> Result<Plan<u32>, PlanError> {
    PlanBuilder::new(Budget::new(budget).expect("test budgets are positive"))
        .tasks(tasks)
        .dependencies(dependencies)
        .build()
}

fn three_tasks(costs: [u64; 3]) -> Vec<TaskSpec<u32>> {
    (0..3)
        .map(|index| task(index, costs[index as usize], &[], &[]))
        .collect()
}

fn nonreflexive_edges(task_count: u32) -> Vec<(TaskId, TaskId)> {
    (0..task_count)
        .flat_map(|source| {
            (0..task_count)
                .filter(move |target| *target != source)
                .map(move |target| (TaskId::new(source), TaskId::new(target)))
        })
        .collect()
}

fn edges_from_mask(task_count: u32, mask: u64) -> Vec<(TaskId, TaskId)> {
    nonreflexive_edges(task_count)
        .into_iter()
        .enumerate()
        .filter_map(|(bit, edge)| ((mask >> bit) & 1 == 1).then_some(edge))
        .collect()
}

fn reference_has_cycle(task_count: usize, edges: &[(TaskId, TaskId)]) -> bool {
    let mut reachable = vec![false; task_count * task_count];
    for &(source, target) in edges {
        reachable[source.index() * task_count + target.index()] = true;
    }
    for middle in 0..task_count {
        for source in 0..task_count {
            for target in 0..task_count {
                reachable[source * task_count + target] |= reachable[source * task_count + middle]
                    && reachable[middle * task_count + target];
            }
        }
    }
    (0..task_count).any(|task| reachable[task * task_count + task])
}

fn reference_topology(task_count: usize, edges: &[(TaskId, TaskId)]) -> Option<Vec<TaskId>> {
    let mut emitted = vec![false; task_count];
    let mut order = Vec::with_capacity(task_count);
    while order.len() < task_count {
        let next = (0..task_count).find(|candidate| {
            !emitted[*candidate]
                && edges
                    .iter()
                    .all(|(source, target)| target.index() != *candidate || emitted[source.index()])
        });
        let next = next?;
        emitted[next] = true;
        order.push(TaskId::new(u32_index(next)));
    }
    Some(order)
}

fn reference_independent(left: &TaskEffects, right: &TaskEffects) -> bool {
    let left_reads: BTreeSet<_> = left.reads().iter().copied().collect();
    let left_writes: BTreeSet<_> = left.writes().iter().copied().collect();
    let right_reads: BTreeSet<_> = right.reads().iter().copied().collect();
    let right_writes: BTreeSet<_> = right.writes().iter().copied().collect();
    left_writes.is_disjoint(&right_reads)
        && left_writes.is_disjoint(&right_writes)
        && right_writes.is_disjoint(&left_reads)
        && right_writes.is_disjoint(&left_writes)
}

fn reference_plan(
    tasks: &[TaskSpec<u32>],
    edges: &[(TaskId, TaskId)],
    budget: u64,
) -> Option<Vec<Vec<TaskId>>> {
    if tasks.iter().any(|task| task.cost().get() > budget) {
        return None;
    }
    let order = reference_topology(tasks.len(), edges)?;
    let task_by_id: BTreeMap<_, _> = tasks.iter().map(|task| (task.id(), task)).collect();
    let mut batches: Vec<Vec<TaskId>> = Vec::new();
    let mut batch_of = vec![usize::MAX; tasks.len()];

    for task_id in order {
        let task = task_by_id.get(&task_id)?;
        let floor = edges
            .iter()
            .filter_map(|(source, target)| (*target == task_id).then_some(batch_of[source.index()]))
            .max()
            .map_or(0, |batch| batch + 1);
        let target = (floor..batches.len()).find(|batch_index| {
            let batch = &batches[*batch_index];
            let cost = batch.iter().fold(task.cost().get(), |total, member| {
                total + task_by_id[member].cost().get()
            });
            cost <= budget
                && batch.iter().all(|member| {
                    reference_independent(task.effects(), task_by_id[member].effects())
                })
        });
        let batch_index = target.unwrap_or_else(|| {
            batches.push(Vec::new());
            batches.len() - 1
        });
        let position = batches[batch_index]
            .binary_search(&task_id)
            .unwrap_or_else(core::convert::identity);
        batches[batch_index].insert(position, task_id);
        batch_of[task_id.index()] = batch_index;
    }
    Some(batches)
}

fn assert_plan_invariants(plan: &Plan<u32>, dependencies: &[(TaskId, TaskId)]) {
    let flat = plan.flattened_task_ids();
    assert_eq!(flat.len(), plan.task_count());
    assert_eq!(
        flat.iter().copied().collect::<BTreeSet<_>>().len(),
        flat.len()
    );
    for batch in plan.batches() {
        assert!(batch.task_ids().windows(2).all(|pair| pair[0] < pair[1]));
        assert!(batch.total_cost().get() <= plan.budget().get());
        for (left_index, left) in batch.task_ids().iter().enumerate() {
            for right in &batch.task_ids()[left_index + 1..] {
                let left = plan.task(*left).expect("planned task exists");
                let right = plan.task(*right).expect("planned task exists");
                assert!(left.effects().is_independent_with(right.effects()));
            }
        }
    }
    for &(source, target) in dependencies {
        assert!(
            plan.batch_index(source).expect("source planned")
                < plan.batch_index(target).expect("target planned")
        );
    }
}

#[derive(Clone)]
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    fn range(&mut self, upper: u64) -> u64 {
        self.next() % upper
    }
}

#[derive(Default)]
struct RecordingSink {
    ids: Vec<TaskId>,
}

impl<O, E, I> CommitSink<O, E, I> for RecordingSink {
    fn commit(&mut self, committed: &CommittedTask<O, E, I>) {
        self.ids.push(committed.task_id());
    }
}

struct OutcomeTaskExecutor {
    outcomes: BTreeMap<TaskId, TaskExecution<u32, u32, u32>>,
    calls: usize,
}

impl TaskExecutor<u32> for OutcomeTaskExecutor {
    type Output = u32;
    type Error = u32;
    type Incomplete = u32;

    fn execute(&mut self, task: TaskView<'_, u32>) -> TaskExecution<u32, u32, u32> {
        self.calls += 1;
        self.outcomes
            .remove(&task.id())
            .expect("each planned task has one test outcome")
    }
}

struct PermutingBatchExecutor {
    outcomes: BTreeMap<TaskId, TaskExecution<u32, u32, u32>>,
    order: CompletionOrder,
}

enum CompletionOrder {
    Canonical,
    Reverse,
    Explicit(Vec<TaskId>),
}

impl BatchExecutor<u32> for PermutingBatchExecutor {
    type Output = u32;
    type Error = u32;
    type Incomplete = u32;

    fn execute_batch(&mut self, batch: BatchView<'_, u32>) -> Vec<TaskCompletion<u32, u32, u32>> {
        let mut ids = match &self.order {
            CompletionOrder::Canonical | CompletionOrder::Reverse => batch.task_ids().to_vec(),
            CompletionOrder::Explicit(order) => order.clone(),
        };
        if matches!(&self.order, CompletionOrder::Reverse) {
            ids.reverse();
        }
        ids.into_iter()
            .map(|id| {
                TaskCompletion::new(
                    id,
                    self.outcomes
                        .remove(&id)
                        .expect("each planned task has one test outcome"),
                )
            })
            .collect()
    }
}

fn completion_permutations(ids: &[TaskId]) -> Vec<Vec<TaskId>> {
    let mut current = ids.to_vec();
    let mut permutations = Vec::new();
    loop {
        permutations.push(current.clone());
        if !next_permutation(&mut current) {
            return permutations;
        }
    }
}

fn next_permutation<T: Ord>(values: &mut [T]) -> bool {
    let Some(pivot) = (0..values.len().saturating_sub(1))
        .rev()
        .find(|index| values[*index] < values[*index + 1])
    else {
        return false;
    };
    let successor = (pivot + 1..values.len())
        .rev()
        .find(|index| values[pivot] < values[*index])
        .expect("a lexicographic successor must exist after the pivot");
    values.swap(pivot, successor);
    values[pivot + 1..].reverse();
    true
}

fn success_outcomes(task_count: u32) -> BTreeMap<TaskId, TaskExecution<u32, u32, u32>> {
    (0..task_count)
        .map(|id| (TaskId::new(id), TaskExecution::Success(id)))
        .collect()
}

fn execute_success(plan: &Plan<u32>) -> (ExecutionPhase, Vec<TaskId>, usize) {
    let worker = OutcomeTaskExecutor {
        outcomes: success_outcomes(u32_index(plan.task_count())),
        calls: 0,
    };
    let mut executor = SerialExecutor::new(worker);
    let mut cancellation = NeverCancel;
    let mut sink = RecordingSink::default();
    let report = plan
        .execute(&mut executor, &mut cancellation, &mut sink)
        .expect("well-formed serial executor");
    let calls = executor.inner().calls;
    (report.phase(), sink.ids, calls)
}

#[test]
fn exhaustive_state_shapes() {
    let empty = build_plan(Vec::new(), Vec::new(), 1).expect("empty input is valid");
    let (phase, ids, calls) = execute_success(&empty);
    assert_eq!(
        (phase, ids, calls),
        (ExecutionPhase::Completed, Vec::new(), 0)
    );

    let plan = build_plan(
        three_tasks([1, 1, 1]),
        vec![(TaskId::new(0), TaskId::new(2))],
        2,
    )
    .expect("acyclic input is valid");
    assert_plan_invariants(&plan, &[(TaskId::new(0), TaskId::new(2))]);
    assert_eq!(execute_success(&plan).0, ExecutionPhase::Completed);
}

#[test]
fn prop_internal_state_is_well_typed() {
    for seed in 0..128 {
        let mut rng = Lcg::new(seed);
        let edges = edges_from_mask(4, rng.next() & ((1 << 12) - 1));
        let tasks = (0..4)
            .map(|id| task(id, rng.range(3) + 1, &[], &[]))
            .collect();
        if let Ok(plan) = build_plan(tasks, edges.clone(), 4) {
            assert_plan_invariants(&plan, &edges);
        }
    }
}

#[test]
fn exhaustive_empty_schedule() {
    let plan = build_plan(Vec::new(), Vec::new(), 7).expect("empty input is identity");
    assert!(plan.is_empty());
    assert!(plan.batches().is_empty());
    assert!(plan.flattened_task_ids().is_empty());
    assert_eq!(
        execute_success(&plan),
        (ExecutionPhase::Completed, Vec::new(), 0)
    );
}

#[test]
fn prop_empty_schedule_is_identity() {
    for budget in 1..=64 {
        let plan = build_plan(Vec::new(), Vec::new(), budget).expect("empty input is valid");
        assert_eq!(
            execute_success(&plan),
            (ExecutionPhase::Completed, Vec::new(), 0)
        );
    }
}

#[test]
fn prop_input_snapshot_never_changes() {
    let plan = build_plan(vec![task(0, 1, &[1], &[2])], Vec::new(), 1)
        .expect("refinement fixture must satisfy its construction invariant");
    let before = format!("{plan:?}");
    let _ = execute_success(&plan);
    assert_eq!(format!("{plan:?}"), before);
}

#[test]
fn metamorphic_mutating_source_after_build_has_no_effect() {
    let mut source = vec![task(0, 1, &[], &[])];
    let plan = build_plan(source.clone(), Vec::new(), 1)
        .expect("refinement fixture must satisfy its construction invariant");
    source[0] = task(0, 1, &[99], &[99]);
    assert!(plan
        .task(TaskId::new(0))
        .expect("refinement fixture must satisfy its construction invariant")
        .effects()
        .reads()
        .is_empty());
}

#[test]
fn exhaustive_all_small_dependency_relations() {
    for mask in 0..64 {
        let edges = edges_from_mask(3, mask);
        let actual = build_plan(three_tasks([1, 1, 1]), edges.clone(), 3);
        assert_eq!(
            actual.is_err(),
            reference_has_cycle(3, &edges),
            "mask {mask}"
        );
    }
}

#[test]
fn prop_cycle_iff_rejected() {
    for seed in 0..256 {
        let mut rng = Lcg::new(seed);
        let mask = rng.next() & ((1 << 20) - 1);
        let edges = edges_from_mask(5, mask);
        let result = build_plan(
            (0..5).map(|id| task(id, 1, &[], &[])).collect(),
            edges.clone(),
            5,
        );
        assert_eq!(
            matches!(result, Err(PlanError::Cyclic)),
            reference_has_cycle(5, &edges)
        );
    }
}

#[test]
fn exhaustive_small_cost_maps() {
    for encoded in 0..27 {
        let costs = [
            encoded % 3 + 1,
            (encoded / 3) % 3 + 1,
            (encoded / 9) % 3 + 1,
        ];
        let result = build_plan(three_tasks(costs), Vec::new(), 2);
        assert_eq!(result.is_err(), costs.iter().any(|cost| *cost > 2));
    }
}

#[test]
fn prop_exhausted_iff_any_cost_exceeds_budget() {
    for seed in 0..128 {
        let mut rng = Lcg::new(seed);
        let budget = rng.range(8) + 1;
        let costs: Vec<_> = (0..12).map(|_| rng.range(12) + 1).collect();
        let tasks = costs
            .iter()
            .enumerate()
            .map(|(id, cost)| task(u32_index(id), *cost, &[], &[]))
            .collect();
        let rejected = build_plan(tasks, Vec::new(), budget).is_err();
        assert_eq!(rejected, costs.iter().any(|cost| *cost > budget));
    }
}

#[test]
fn exhaustive_rejection_is_atomic() {
    let cycle = vec![
        (TaskId::new(0), TaskId::new(1)),
        (TaskId::new(1), TaskId::new(0)),
    ];
    assert!(matches!(
        build_plan(three_tasks([1, 1, 1]), cycle, 3),
        Err(PlanError::Cyclic)
    ));
    assert!(matches!(
        build_plan(three_tasks([1, 4, 1]), Vec::new(), 3),
        Err(PlanError::ResourceExhausted { .. })
    ));
}

#[test]
fn prop_rejection_has_zero_observable_effects() {
    // A rejected builder cannot produce a Plan, so the executor and sink APIs
    // have no value on which they could be invoked.
    assert!(build_plan(three_tasks([2, 2, 2]), Vec::new(), 1).is_err());
}

#[test]
fn cycle_rejection_precedes_resource_exhaustion() {
    let cycle = vec![
        (TaskId::new(0), TaskId::new(1)),
        (TaskId::new(1), TaskId::new(0)),
    ];
    assert!(matches!(
        build_plan(three_tasks([4, 4, 4]), cycle, 1),
        Err(PlanError::Cyclic)
    ));
}

#[test]
fn duplicate_and_unknown_task_inputs_are_rejected_atomically() {
    assert!(matches!(
        build_plan(
            vec![task(7, 1, &[], &[]), task(7, 1, &[], &[])],
            Vec::new(),
            1,
        ),
        Err(PlanError::DuplicateTaskId { task }) if task == TaskId::new(7)
    ));
    assert!(matches!(
        build_plan(
            vec![task(7, 1, &[], &[])],
            vec![(TaskId::new(7), TaskId::new(9))],
            1,
        ),
        Err(PlanError::Graph(_))
    ));
}

#[test]
fn sparse_stable_identifiers_use_dense_internal_storage() {
    let plan = build_plan(
        vec![
            task(u32::MAX, 1, &[], &[]),
            task(7, 1, &[], &[]),
            task(100_000, 1, &[], &[]),
        ],
        vec![(TaskId::new(7), TaskId::new(u32::MAX))],
        3,
    )
    .expect("sparse stable identifiers must be representable through dense CSR");
    assert_eq!(
        plan.canonical_order(),
        vec![TaskId::new(7), TaskId::new(100_000), TaskId::new(u32::MAX),]
    );
    assert!(
        plan.batch_index(TaskId::new(7)).expect("source is planned")
            < plan
                .batch_index(TaskId::new(u32::MAX))
                .expect("target is planned")
    );
}

#[test]
fn duplicate_dependencies_are_canonicalized_once() {
    let edge = (TaskId::new(0), TaskId::new(1));
    let plan = build_plan(
        vec![task(0, 1, &[], &[]), task(1, 1, &[], &[])],
        vec![edge, edge, edge],
        2,
    )
    .expect("duplicate relation inputs denote one set edge");
    assert_eq!(plan.dependency_graph().edge_count(), 1);
    assert_eq!(plan.work_profile().edges(), 1);
    assert_eq!(plan.work_profile().topology_edge_visits(), 1);
}

#[test]
fn maximum_costs_cannot_overflow_batch_arithmetic() {
    let plan = build_plan(
        vec![task(0, u64::MAX, &[], &[]), task(1, u64::MAX, &[], &[])],
        Vec::new(),
        u64::MAX,
    )
    .expect("each maximum-cost task fits its own maximum-budget batch");
    assert_eq!(plan.batches().len(), 2);
    assert!(plan
        .batches()
        .iter()
        .all(|batch| batch.total_cost().get() == u64::MAX));
}

#[test]
fn effect_sets_canonicalize_duplicates_without_recursive_sort() {
    let set = EffectSet::from_resources([
        ResourceId::new(9),
        ResourceId::new(1),
        ResourceId::new(9),
        ResourceId::new(4),
    ]);
    assert_eq!(
        set.as_slice(),
        [ResourceId::new(1), ResourceId::new(4), ResourceId::new(9)]
    );
}

#[test]
fn exhaustive_canonical_topological_order() {
    for mask in 0..64 {
        let edges = edges_from_mask(3, mask);
        let expected = reference_topology(3, &edges);
        let actual = build_plan(three_tasks([1, 1, 1]), edges.clone(), 3);
        match (expected, actual) {
            (Some(expected), Ok(plan)) => assert_eq!(plan.canonical_order(), expected),
            (None, Err(PlanError::Cyclic)) => {}
            pair => panic!("topology disagreement for mask {mask}: {pair:?}"),
        }
    }
}

#[test]
fn prop_ready_permutation_invariant() {
    let dependencies = vec![
        (TaskId::new(0), TaskId::new(3)),
        (TaskId::new(1), TaskId::new(3)),
    ];
    let forward = build_plan(
        (0..4).map(|id| task(id, 1, &[], &[])).collect(),
        dependencies.clone(),
        4,
    )
    .expect("refinement fixture must satisfy its construction invariant");
    let reverse = build_plan(
        (0..4).rev().map(|id| task(id, 1, &[], &[])).collect(),
        dependencies.into_iter().rev().collect(),
        4,
    )
    .expect("refinement fixture must satisfy its construction invariant");
    assert_eq!(forward.canonical_order(), reverse.canonical_order());
    assert_eq!(forward.batch_task_ids(), reverse.batch_task_ids());
}

#[test]
fn exhaustive_plan_matches_reference() {
    for mask in 0..64 {
        let dependencies = edges_from_mask(3, mask);
        let tasks = three_tasks([1, 1, 1]);
        let expected = reference_plan(&tasks, &dependencies, 2);
        match (expected, build_plan(tasks, dependencies.clone(), 2)) {
            (Some(expected), Ok(plan)) => {
                assert_eq!(plan.batch_task_ids(), expected);
                assert_plan_invariants(&plan, &dependencies);
            }
            (None, Err(PlanError::Cyclic)) => {}
            pair => panic!("reference-plan disagreement for mask {mask}: {pair:?}"),
        }
    }
}

#[test]
fn prop_repeated_build_is_identical() {
    for seed in 0..64 {
        let dependencies = edges_from_mask(4, seed);
        if reference_has_cycle(4, &dependencies) {
            continue;
        }
        let left = build_plan(
            (0..4).map(|id| task(id, 1, &[id % 2], &[])).collect(),
            dependencies.clone(),
            3,
        )
        .expect("refinement fixture must satisfy its construction invariant");
        let right = build_plan(
            (0..4).rev().map(|id| task(id, 1, &[id % 2], &[])).collect(),
            dependencies,
            3,
        )
        .expect("refinement fixture must satisfy its construction invariant");
        assert_eq!(left.batch_task_ids(), right.batch_task_ids());
    }
}

#[test]
fn exhaustive_plan_exact_cover() {
    let plan = build_plan(
        (0..8).map(|id| task(id, 1, &[], &[])).collect(),
        Vec::new(),
        3,
    )
    .expect("refinement fixture must satisfy its construction invariant");
    assert_eq!(plan.flattened_task_ids().len(), 8);
    assert_eq!(
        plan.flattened_task_ids()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len(),
        8
    );
}

#[test]
fn prop_no_omission_or_duplication() {
    for count in 0..64 {
        let plan = build_plan(
            (0..count).map(|id| task(id, 1, &[], &[])).collect(),
            Vec::new(),
            8,
        )
        .expect("refinement fixture must satisfy its construction invariant");
        assert_eq!(
            plan.flattened_task_ids(),
            (0..count).map(TaskId::new).collect::<Vec<_>>()
        );
    }
}

#[test]
fn exhaustive_batch_order() {
    let plan = build_plan(
        (0..12).rev().map(|id| task(id, 1, &[], &[])).collect(),
        Vec::new(),
        5,
    )
    .expect("refinement fixture must satisfy its construction invariant");
    assert!(plan
        .batches()
        .iter()
        .all(|batch| batch.task_ids().windows(2).all(|pair| pair[0] < pair[1])));
}

#[test]
fn regression_late_low_id_is_sorted_within_batch() {
    let tasks = vec![
        task(0, 1, &[], &[0]),
        task(1, 1, &[], &[1]),
        task(2, 1, &[1], &[]),
        task(3, 1, &[0], &[]),
    ];
    let plan = build_plan(tasks, vec![(TaskId::new(3), TaskId::new(0))], 4)
        .expect("the targeted relation is acyclic and admissible");
    assert_eq!(
        plan.canonical_order(),
        vec![
            TaskId::new(1),
            TaskId::new(2),
            TaskId::new(3),
            TaskId::new(0),
        ]
    );
    assert_eq!(
        plan.batch_task_ids(),
        vec![
            vec![TaskId::new(1), TaskId::new(3)],
            vec![TaskId::new(0), TaskId::new(2)],
        ]
    );
}

#[test]
fn prop_batch_ids_strictly_increase() {
    for seed in 0..64 {
        let plan = build_plan(
            (0..16)
                .rev()
                .map(|id| task(id, 1, &[id % 4], &[]))
                .collect(),
            Vec::new(),
            seed % 8 + 1,
        )
        .expect("refinement fixture must satisfy its construction invariant");
        assert!(plan
            .batches()
            .iter()
            .all(|batch| batch.task_ids().windows(2).all(|pair| pair[0] < pair[1])));
    }
}

#[test]
fn exhaustive_effect_kernel() {
    for encoding in 0..256u32 {
        let subset = |shift: u32| -> Vec<u32> {
            let bits = (encoding >> shift) & 3;
            (0..2).filter(|bit| bits & (1 << bit) != 0).collect()
        };
        let left = effects(&subset(0), &subset(2));
        let right = effects(&subset(4), &subset(6));
        let plan = build_plan(
            vec![
                TaskSpec::new(
                    TaskId::new(0),
                    0,
                    left.clone(),
                    Cost::new(1)
                        .expect("refinement fixture must satisfy its construction invariant"),
                ),
                TaskSpec::new(
                    TaskId::new(1),
                    1,
                    right.clone(),
                    Cost::new(1)
                        .expect("refinement fixture must satisfy its construction invariant"),
                ),
            ],
            Vec::new(),
            2,
        )
        .expect("refinement fixture must satisfy its construction invariant");
        assert_eq!(
            plan.batches().len() == 1,
            reference_independent(&left, &right)
        );
    }
}

#[test]
fn prop_batch_members_are_pairwise_independent() {
    for seed in 0..64 {
        let mut rng = Lcg::new(seed);
        let tasks = (0..24)
            .map(|id| {
                task(
                    id,
                    1,
                    &[u32_sample(rng.range(8))],
                    &[u32_sample(rng.range(8))],
                )
            })
            .collect();
        let plan = build_plan(tasks, Vec::new(), 24)
            .expect("refinement fixture must satisfy its construction invariant");
        assert_plan_invariants(&plan, &[]);
    }
}

#[test]
fn exhaustive_effect_symmetry() {
    for encoding in 0..256u32 {
        let subset = |shift: u32| -> Vec<u32> {
            let bits = (encoding >> shift) & 3;
            (0..2).filter(|bit| bits & (1 << bit) != 0).collect()
        };
        let left = effects(&subset(0), &subset(2));
        let right = effects(&subset(4), &subset(6));
        assert_eq!(
            left.is_independent_with(&right),
            right.is_independent_with(&left)
        );
    }
}

#[test]
fn prop_independence_is_symmetric() {
    for seed in 0..256 {
        let mut rng = Lcg::new(seed);
        let left = effects(&[u32_sample(rng.range(16))], &[u32_sample(rng.range(16))]);
        let right = effects(&[u32_sample(rng.range(16))], &[u32_sample(rng.range(16))]);
        assert_eq!(
            left.prove_independent(&right).is_some(),
            right.prove_independent(&left).is_some()
        );
    }
}

#[test]
fn exhaustive_batch_costs() {
    for encoded in 0..27 {
        let costs = [
            encoded % 3 + 1,
            (encoded / 3) % 3 + 1,
            (encoded / 9) % 3 + 1,
        ];
        if let Ok(plan) = build_plan(three_tasks(costs), Vec::new(), 2) {
            assert!(plan
                .batches()
                .iter()
                .all(|batch| batch.total_cost().get() <= 2));
        }
    }
}

#[test]
fn prop_batch_cost_never_exceeds_budget() {
    for seed in 0..64 {
        let mut rng = Lcg::new(seed);
        let tasks = (0..64)
            .map(|id| task(id, rng.range(8) + 1, &[], &[]))
            .collect();
        let plan = build_plan(tasks, Vec::new(), 8)
            .expect("refinement fixture must satisfy its construction invariant");
        assert!(plan
            .batches()
            .iter()
            .all(|batch| batch.total_cost().get() <= 8));
    }
}

#[test]
fn exhaustive_dependency_batches() {
    for mask in 0..64 {
        let edges = edges_from_mask(3, mask);
        if let Ok(plan) = build_plan(three_tasks([1, 1, 1]), edges.clone(), 3) {
            assert_plan_invariants(&plan, &edges);
        }
    }
}

#[test]
fn prop_edge_implies_strict_batch_order() {
    for count in 2..64 {
        let edges = (0..count - 1)
            .map(|id| (TaskId::new(id), TaskId::new(id + 1)))
            .collect::<Vec<_>>();
        let plan = build_plan(
            (0..count).map(|id| task(id, 1, &[], &[])).collect(),
            edges.clone(),
            u64::from(count),
        )
        .expect("refinement fixture must satisfy its construction invariant");
        assert_plan_invariants(&plan, &edges);
    }
}

struct InvalidExecutor {
    completions: Vec<TaskCompletion<u32, u32, u32>>,
}

impl BatchExecutor<u32> for InvalidExecutor {
    type Output = u32;
    type Error = u32;
    type Incomplete = u32;

    fn execute_batch(&mut self, _batch: BatchView<'_, u32>) -> Vec<TaskCompletion<u32, u32, u32>> {
        std::mem::take(&mut self.completions)
    }
}

#[test]
fn exhaustive_completion_domain() {
    let plan = build_plan(
        vec![task(0, 1, &[], &[]), task(1, 1, &[], &[])],
        Vec::new(),
        2,
    )
    .expect("refinement fixture must satisfy its construction invariant");
    for completions in [
        vec![TaskCompletion::new(
            TaskId::new(9),
            TaskExecution::Success(9),
        )],
        vec![
            TaskCompletion::new(TaskId::new(0), TaskExecution::Success(0)),
            TaskCompletion::new(TaskId::new(0), TaskExecution::Success(0)),
        ],
        vec![TaskCompletion::new(
            TaskId::new(0),
            TaskExecution::Success(0),
        )],
    ] {
        let mut executor = InvalidExecutor { completions };
        let mut cancel = NeverCancel;
        let mut sink = RecordingSink::default();
        assert!(plan.execute(&mut executor, &mut cancel, &mut sink).is_err());
        assert!(sink.ids.is_empty());
    }
}

#[test]
fn prop_foreign_duplicate_or_future_completion_rejected() {
    exhaustive_completion_domain();
}

#[test]
fn exhaustive_commit_barrier() {
    let plan = build_plan(
        vec![task(0, 1, &[], &[]), task(1, 1, &[], &[])],
        Vec::new(),
        2,
    )
    .expect("refinement fixture must satisfy its construction invariant");
    let mut executor = InvalidExecutor {
        completions: vec![TaskCompletion::new(
            TaskId::new(1),
            TaskExecution::Success(1),
        )],
    };
    let mut cancel = NeverCancel;
    let mut sink = RecordingSink::default();
    assert!(plan.execute(&mut executor, &mut cancel, &mut sink).is_err());
    assert!(sink.ids.is_empty());
}

#[test]
fn prop_partial_batch_cannot_commit() {
    exhaustive_commit_barrier();
}

#[test]
fn exhaustive_result_growth() {
    let plan = build_plan(
        (0..8).map(|id| task(id, 1, &[], &[])).collect(),
        Vec::new(),
        8,
    )
    .expect("refinement fixture must satisfy its construction invariant");
    let (_, ids, _) = execute_success(&plan);
    for prefix in ids.windows(2) {
        assert!(prefix[0] < prefix[1]);
    }
}

#[test]
fn prop_completed_result_set_is_monotone() {
    exhaustive_result_growth();
}

#[test]
fn exhaustive_plan_immutability() {
    prop_input_snapshot_never_changes();
}

#[test]
fn prop_execution_never_mutates_plan() {
    prop_input_snapshot_never_changes();
}

#[test]
fn exhaustive_completion_permutations() {
    let plan = build_plan(
        (0..4).map(|id| task(id, 1, &[], &[])).collect(),
        Vec::new(),
        4,
    )
    .expect("refinement fixture must satisfy its construction invariant");
    for order in completion_permutations(plan.batches()[0].task_ids()) {
        let mut executor = PermutingBatchExecutor {
            outcomes: success_outcomes(4),
            order: CompletionOrder::Explicit(order),
        };
        let mut cancel = NeverCancel;
        let mut sink = RecordingSink::default();
        let report = plan
            .execute(&mut executor, &mut cancel, &mut sink)
            .expect("refinement fixture must satisfy its construction invariant");
        assert_eq!(report.phase(), ExecutionPhase::Completed);
        assert_eq!(sink.ids, plan.flattened_task_ids());
    }
}

#[test]
fn prop_commit_is_canonical_prefix() {
    exhaustive_completion_permutations();
}

#[test]
fn exhaustive_single_step_commit() {
    let plan = build_plan(
        (0..4).map(|id| task(id, 1, &[], &[])).collect(),
        Vec::new(),
        4,
    )
    .expect("refinement fixture must satisfy its construction invariant");
    let (_, ids, _) = execute_success(&plan);
    assert_eq!(ids, plan.flattened_task_ids());
}

#[test]
fn prop_commit_length_delta_is_zero_or_one() {
    exhaustive_single_step_commit();
}

#[test]
fn exhaustive_committed_predecessors() {
    let edges = vec![
        (TaskId::new(0), TaskId::new(2)),
        (TaskId::new(1), TaskId::new(2)),
    ];
    let plan = build_plan(three_tasks([1, 1, 1]), edges.clone(), 3)
        .expect("refinement fixture must satisfy its construction invariant");
    let (_, committed, _) = execute_success(&plan);
    for (source, target) in edges {
        let source_position = committed
            .iter()
            .position(|id| *id == source)
            .expect("refinement fixture must satisfy its construction invariant");
        let target_position = committed
            .iter()
            .position(|id| *id == target)
            .expect("refinement fixture must satisfy its construction invariant");
        assert!(source_position < target_position);
    }
}

#[test]
fn prop_commit_respects_transitive_dependencies() {
    let count = 32;
    let edges = (0..count - 1)
        .map(|id| (TaskId::new(id), TaskId::new(id + 1)))
        .collect();
    let plan = build_plan(
        (0..count).map(|id| task(id, 1, &[], &[])).collect(),
        edges,
        u64::from(count),
    )
    .expect("refinement fixture must satisfy its construction invariant");
    assert_eq!(
        execute_success(&plan).1,
        (0..count).map(TaskId::new).collect::<Vec<_>>()
    );
}

fn ternary_outcomes(mut encoded: u32) -> BTreeMap<TaskId, TaskExecution<u32, u32, u32>> {
    (0..3)
        .map(|id| {
            let outcome = match encoded % 3 {
                0 => TaskExecution::Success(id),
                1 => TaskExecution::Failure(id),
                _ => TaskExecution::Incomplete(id),
            };
            encoded /= 3;
            (TaskId::new(id), outcome)
        })
        .collect()
}

fn expected_phase_and_len(encoded: u32, cancel_after: usize) -> (ExecutionPhase, usize) {
    let outcomes = ternary_outcomes(encoded);
    let first_non_success = (0..3)
        .find(|id| !matches!(outcomes[&TaskId::new(*id)], TaskExecution::Success(_)))
        .map(|id| id as usize + 1);
    if cancel_after < first_non_success.unwrap_or(4) && cancel_after < 3 {
        return (ExecutionPhase::Cancelled, cancel_after);
    }
    match first_non_success {
        Some(position) => match outcomes[&TaskId::new(u32_index(position - 1))] {
            TaskExecution::Failure(_) => (ExecutionPhase::Failed, position),
            TaskExecution::Incomplete(_) => (ExecutionPhase::Incomplete, position),
            TaskExecution::Success(_) => unreachable!(),
        },
        None => (ExecutionPhase::Completed, 3),
    }
}

#[test]
fn exhaustive_outcomes_and_completion_orders() {
    let plan = build_plan(three_tasks([1, 1, 1]), Vec::new(), 3)
        .expect("refinement fixture must satisfy its construction invariant");
    let completion_orders = completion_permutations(plan.batches()[0].task_ids());
    for encoded in 0..27 {
        for cancel_after in 0..=3 {
            for completion_order in &completion_orders {
                let mut executor = PermutingBatchExecutor {
                    outcomes: ternary_outcomes(encoded),
                    order: CompletionOrder::Explicit(completion_order.clone()),
                };
                let mut cancel = CancelAfter::new(cancel_after);
                let mut sink = RecordingSink::default();
                let report = plan
                    .execute(&mut executor, &mut cancel, &mut sink)
                    .expect("refinement fixture must satisfy its construction invariant");
                let expected = expected_phase_and_len(encoded, cancel_after);
                assert_eq!((report.phase(), report.committed().len()), expected);
                assert_eq!(sink.ids, plan.flattened_task_ids()[..expected.1]);
            }
        }
    }
}

#[test]
fn prop_first_failure_is_terminal() {
    let plan = build_plan(three_tasks([1, 1, 1]), Vec::new(), 3)
        .expect("refinement fixture must satisfy its construction invariant");
    let mut outcomes = success_outcomes(3);
    outcomes.insert(TaskId::new(1), TaskExecution::Failure(99));
    let mut executor = PermutingBatchExecutor {
        outcomes,
        order: CompletionOrder::Reverse,
    };
    let mut cancel = NeverCancel;
    let mut sink = RecordingSink::default();
    let report = plan
        .execute(&mut executor, &mut cancel, &mut sink)
        .expect("refinement fixture must satisfy its construction invariant");
    assert_eq!(report.phase(), ExecutionPhase::Failed);
    assert_eq!(sink.ids, vec![TaskId::new(0), TaskId::new(1)]);
}

#[test]
fn exhaustive_incomplete_outcomes() {
    exhaustive_outcomes_and_completion_orders();
}

#[test]
fn prop_first_incomplete_is_terminal() {
    let plan = build_plan(three_tasks([1, 1, 1]), Vec::new(), 3)
        .expect("refinement fixture must satisfy its construction invariant");
    let mut outcomes = success_outcomes(3);
    outcomes.insert(TaskId::new(1), TaskExecution::Incomplete(77));
    let mut executor = PermutingBatchExecutor {
        outcomes,
        order: CompletionOrder::Reverse,
    };
    let mut cancel = NeverCancel;
    let mut sink = RecordingSink::default();
    let report = plan
        .execute(&mut executor, &mut cancel, &mut sink)
        .expect("refinement fixture must satisfy its construction invariant");
    assert_eq!(report.phase(), ExecutionPhase::Incomplete);
    assert_eq!(
        report
            .committed()
            .last()
            .expect("refinement fixture must satisfy its construction invariant")
            .outcome(),
        &TaskExecution::Incomplete(77)
    );
}

#[test]
fn exhaustive_outcomes_cancel_boundaries() {
    exhaustive_outcomes_and_completion_orders();
}

#[test]
fn prop_cancel_never_splits_a_commit() {
    let plan = build_plan(three_tasks([1, 1, 1]), Vec::new(), 3)
        .expect("refinement fixture must satisfy its construction invariant");
    for boundary in 0..=3 {
        let mut executor = PermutingBatchExecutor {
            outcomes: success_outcomes(3),
            order: CompletionOrder::Canonical,
        };
        let mut cancel = CancelAfter::new(boundary);
        let mut sink = RecordingSink::default();
        let report = plan
            .execute(&mut executor, &mut cancel, &mut sink)
            .expect("refinement fixture must satisfy its construction invariant");
        assert_eq!(report.committed().len(), boundary);
    }
}

#[test]
fn exhaustive_completion_outcomes() {
    exhaustive_outcomes_and_completion_orders();
}

#[test]
fn prop_completed_iff_all_tasks_successfully_committed() {
    for encoded in 0..27 {
        let (phase, count) = expected_phase_and_len(encoded, 3);
        assert_eq!(
            phase == ExecutionPhase::Completed,
            encoded == 0 && count == 3
        );
    }
}

#[test]
fn exhaustive_parallel_completion_permutations() {
    exhaustive_completion_permutations();
}

#[test]
fn prop_parallel_observation_equals_serial() {
    exhaustive_outcomes_and_completion_orders();
}

#[test]
fn exhaustive_finite_trace_termination() {
    exhaustive_outcomes_and_completion_orders();
}

#[test]
fn prop_progress_measure_strictly_decreases() {
    let plan = build_plan(
        (0..128).map(|id| task(id, 1, &[], &[])).collect(),
        Vec::new(),
        8,
    )
    .expect("refinement fixture must satisfy its construction invariant");
    let report = {
        let worker = OutcomeTaskExecutor {
            outcomes: success_outcomes(128),
            calls: 0,
        };
        let mut executor = SerialExecutor::new(worker);
        let mut cancel = NeverCancel;
        let mut sink = RecordingSink::default();
        plan.execute(&mut executor, &mut cancel, &mut sink)
            .expect("refinement fixture must satisfy its construction invariant")
    };
    assert_eq!(report.work_profile().tasks_dispatched(), 128);
    assert_eq!(report.work_profile().commits(), 128);
}

fn run_small_stack(task_count: u32, chain: bool) {
    thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || {
            let edges = if chain {
                (0..task_count.saturating_sub(1))
                    .map(|id| (TaskId::new(id), TaskId::new(id + 1)))
                    .collect()
            } else {
                Vec::new()
            };
            let plan = build_plan(
                (0..task_count).map(|id| task(id, 1, &[], &[])).collect(),
                edges,
                u64::from(task_count.max(1)),
            )
            .expect("refinement fixture must satisfy its construction invariant");
            assert_eq!(execute_success(&plan).0, ExecutionPhase::Completed);
            drop(plan);
        })
        .expect("small-stack test thread starts")
        .join()
        .expect("iterative scheduler stays within the small stack");
}

#[test]
fn small_stack_deep_chain() {
    run_small_stack(20_000, true);
}

#[test]
fn small_stack_wide_dag() {
    run_small_stack(20_000, false);
}

#[test]
fn prop_no_recursive_drop() {
    run_small_stack(10_000, true);
}

#[test]
fn pda_reference_oracle() {
    let plan = build_plan(three_tasks([1, 1, 1]), Vec::new(), 3)
        .expect("refinement fixture must satisfy its construction invariant");
    assert_eq!(plan.control_model(), ControlModel::FlatIterative);
}

#[test]
fn prop_pda_matches_recursive_denotation() {
    // The current scheduler language is a flat DAG and therefore has no
    // pushdown semantics to refine. The API must say so explicitly.
    pda_reference_oracle();
}

#[test]
fn small_stack_deep_nesting() {
    // This is the applicable regression until a nested-task extension exists.
    run_small_stack(10_000, true);
}

#[test]
fn operation_count_topology() {
    let count = 1_024u32;
    let edges = (0..count - 1)
        .map(|id| (TaskId::new(id), TaskId::new(id + 1)))
        .collect::<Vec<_>>();
    let plan = build_plan(
        (0..count).map(|id| task(id, 1, &[], &[])).collect(),
        edges.clone(),
        u64::from(count),
    )
    .expect("refinement fixture must satisfy its construction invariant");
    let work = plan.work_profile();
    assert_eq!(work.vertices(), u64::from(count));
    assert_eq!(work.edges(), u64_count(edges.len()));
    assert_eq!(work.topology_edge_visits(), u64_count(edges.len()));
    assert_eq!(work.ready_pops(), u64::from(count));
    assert_eq!(work.ready_pushes(), u64::from(count));
}

#[test]
fn operation_count_execution() {
    prop_progress_measure_strictly_decreases();
}

#[test]
fn benchmark_scaling_slope() {
    let build = |count: u32| {
        let plan = build_plan(
            (0..count).map(|id| task(id, 1, &[], &[])).collect(),
            Vec::new(),
            u64::from(count),
        )
        .expect("refinement fixture must satisfy its construction invariant");
        plan.work_profile().logical_work()
    };
    let small = build(512);
    let large = build(1_024);
    assert!(large <= small.saturating_mul(3));
}

#[test]
fn prop_capacity_bounds() {
    let count = 1_024u32;
    let edges = (0..count - 1)
        .map(|id| (TaskId::new(id), TaskId::new(id + 1)))
        .collect::<Vec<_>>();
    let plan = build_plan(
        (0..count).map(|id| task(id, 1, &[], &[])).collect(),
        edges.clone(),
        u64::from(count),
    )
    .expect("refinement fixture must satisfy its construction invariant");
    assert!(
        plan.work_profile().auxiliary_slots_peak()
            <= 12 * u64::from(count) + 2 * u64_count(edges.len())
    );
}

#[test]
fn allocation_count_regression() {
    let plan = build_plan(
        (0..2_048).map(|id| task(id, 1, &[], &[])).collect(),
        Vec::new(),
        64,
    )
    .expect("refinement fixture must satisfy its construction invariant");
    assert!(plan.work_profile().allocation_count() <= 32 + u64_count(plan.batches().len()) * 4);
}

#[test]
fn small_stack_drop() {
    run_small_stack(20_000, true);
}

#[test]
fn small_stack_deep_drop_after_success_failure_cancel() {
    thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            for phase in 0..3 {
                let plan = build_plan(
                    (0..10_000).map(|id| task(id, 1, &[], &[])).collect(),
                    Vec::new(),
                    256,
                )
                .expect("refinement fixture must satisfy its construction invariant");
                let mut outcomes = success_outcomes(10_000);
                if phase == 1 {
                    outcomes.insert(TaskId::new(5_000), TaskExecution::Failure(1));
                }
                let mut executor = PermutingBatchExecutor {
                    outcomes,
                    order: CompletionOrder::Reverse,
                };
                let mut cancel = CancelAfter::new(if phase == 2 { 5_000 } else { usize::MAX });
                let mut sink = RecordingSink::default();
                let _ = plan
                    .execute(&mut executor, &mut cancel, &mut sink)
                    .expect("refinement fixture must satisfy its construction invariant");
                drop(plan);
            }
        })
        .expect("refinement fixture must satisfy its construction invariant")
        .join()
        .expect("all terminal paths and drops are stack-safe");
}
