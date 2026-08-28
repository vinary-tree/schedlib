use core::cmp::Reverse;
use core::fmt;
use std::collections::{BinaryHeap, HashMap, HashSet};

use libvgraph::{CsrGraph, DenseId, GraphError};

use crate::{Budget, Cost, ResourceId, TaskEffects, TaskId};

const SEARCH_STACK_CAPACITY: usize = 130;

/// One immutable task supplied to a schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec<T> {
    id: TaskId,
    payload: T,
    effects: TaskEffects,
    cost: Cost,
}

impl<T> TaskSpec<T> {
    /// Creates one task with stable identity, payload, effects, and cost.
    #[must_use]
    pub const fn new(id: TaskId, payload: T, effects: TaskEffects, cost: Cost) -> Self {
        Self {
            id,
            payload,
            effects,
            cost,
        }
    }

    /// Returns the stable task identifier.
    #[must_use]
    pub const fn id(&self) -> TaskId {
        self.id
    }

    /// Returns the caller-owned payload.
    #[must_use]
    pub const fn payload(&self) -> &T {
        &self.payload
    }

    /// Returns canonical effect metadata.
    #[must_use]
    pub const fn effects(&self) -> &TaskEffects {
        &self.effects
    }

    /// Returns the positive task cost.
    #[must_use]
    pub const fn cost(&self) -> Cost {
        self.cost
    }
}

/// One immutable independent, budget-admissible plan batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanBatch {
    task_ids: Vec<TaskId>,
    total_cost: Cost,
}

impl PlanBatch {
    /// Returns members in strictly increasing stable-identifier order.
    #[must_use]
    pub fn task_ids(&self) -> &[TaskId] {
        &self.task_ids
    }

    /// Returns the batch's total positive cost.
    #[must_use]
    pub const fn total_cost(&self) -> Cost {
        self.total_cost
    }
}

/// Scheduler control representation selected for the current task language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlModel {
    /// Flat graph planning and execution use iterative queues and cursors.
    FlatIterative,
}

/// Exact logical event counts and proven storage bounds for one plan build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlanWorkProfile {
    vertices: u64,
    edges: u64,
    topology_edge_visits: u64,
    ready_pushes: u64,
    ready_pops: u64,
    batch_probes: u64,
    conflict_lookups: u64,
    capacity_tree_nodes: u64,
    batch_sort_work_upper_bound: u64,
    allocation_count: u64,
    auxiliary_slots_peak: u64,
}

impl PlanWorkProfile {
    /// Returns the canonical task count.
    #[must_use]
    pub const fn vertices(self) -> u64 {
        self.vertices
    }

    /// Returns the canonical dependency-edge count after deduplication.
    #[must_use]
    pub const fn edges(self) -> u64 {
        self.edges
    }

    /// Returns exact successor-edge visits by Kahn topology.
    #[must_use]
    pub const fn topology_edge_visits(self) -> u64 {
        self.topology_edge_visits
    }

    /// Returns exact insertions into the stable ready heap.
    #[must_use]
    pub const fn ready_pushes(self) -> u64 {
        self.ready_pushes
    }

    /// Returns exact removals from the stable ready heap.
    #[must_use]
    pub const fn ready_pops(self) -> u64 {
        self.ready_pops
    }

    /// Returns capacity-admissible batches tested for effect conflicts.
    #[must_use]
    pub const fn batch_probes(self) -> u64 {
        self.batch_probes
    }

    /// Returns sparse aggregate-effect membership lookups.
    #[must_use]
    pub const fn conflict_lookups(self) -> u64 {
        self.conflict_lookups
    }

    /// Returns max-capacity segment-tree nodes inspected.
    #[must_use]
    pub const fn capacity_tree_nodes(self) -> u64 {
        self.capacity_tree_nodes
    }

    /// Returns the heap-sort comparison-work upper bound used to canonicalize
    /// all batch members after assignment.
    #[must_use]
    pub const fn batch_sort_work_upper_bound(self) -> u64 {
        self.batch_sort_work_upper_bound
    }

    /// Returns the number of scheduler-owned logical allocation sites.
    #[must_use]
    pub const fn allocation_count(self) -> u64 {
        self.allocation_count
    }

    /// Returns the proven peak upper bound for scheduler-owned temporary slots.
    #[must_use]
    pub const fn auxiliary_slots_peak(self) -> u64 {
        self.auxiliary_slots_peak
    }

    /// Returns the complete deterministic logical-work charge.
    #[must_use]
    pub const fn logical_work(self) -> u64 {
        self.vertices
            .saturating_add(self.edges)
            .saturating_add(self.topology_edge_visits)
            .saturating_add(self.ready_pushes)
            .saturating_add(self.ready_pops)
            .saturating_add(self.batch_probes)
            .saturating_add(self.conflict_lookups)
            .saturating_add(self.capacity_tree_nodes)
            .saturating_add(self.batch_sort_work_upper_bound)
    }
}

/// Immutable deterministic schedule and its canonical dependency graph.
#[derive(Debug)]
pub struct Plan<T> {
    budget: Budget,
    tasks: Vec<TaskSpec<T>>,
    task_index: HashMap<TaskId, usize>,
    dependency_graph: CsrGraph<TaskId>,
    canonical_order: Vec<TaskId>,
    batches: Vec<PlanBatch>,
    batch_of: Vec<usize>,
    position_in_batch: Vec<usize>,
    work_profile: PlanWorkProfile,
}

impl<T> Plan<T> {
    /// Returns whether the schedule contains no tasks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Returns the number of tasks.
    #[must_use]
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Returns the immutable resource budget.
    #[must_use]
    pub const fn budget(&self) -> Budget {
        self.budget
    }

    /// Returns immutable deterministic batches.
    #[must_use]
    pub fn batches(&self) -> &[PlanBatch] {
        &self.batches
    }

    /// Returns the stable minimum-ready Kahn order used for assignment.
    #[must_use]
    pub fn canonical_order(&self) -> &[TaskId] {
        &self.canonical_order
    }

    /// Materializes task identifiers grouped by canonical batch.
    #[must_use]
    pub fn batch_task_ids(&self) -> Vec<Vec<TaskId>> {
        self.batches
            .iter()
            .map(|batch| batch.task_ids.clone())
            .collect()
    }

    /// Materializes the canonical ordered-commit sequence.
    #[must_use]
    pub fn flattened_task_ids(&self) -> Vec<TaskId> {
        let mut flattened = Vec::with_capacity(self.tasks.len());
        for batch in &self.batches {
            flattened.extend_from_slice(&batch.task_ids);
        }
        flattened
    }

    /// Returns one task by stable identifier.
    #[must_use]
    pub fn task(&self, id: TaskId) -> Option<&TaskSpec<T>> {
        self.task_index
            .get(&id)
            .and_then(|index| self.tasks.get(*index))
    }

    /// Returns the zero-based batch containing a task.
    #[must_use]
    pub fn batch_index(&self, id: TaskId) -> Option<usize> {
        self.task_index
            .get(&id)
            .and_then(|index| self.batch_of.get(*index).copied())
    }

    /// Returns the canonical libvgraph dependency representation.
    #[must_use]
    pub const fn dependency_graph(&self) -> &CsrGraph<TaskId> {
        &self.dependency_graph
    }

    /// Returns exact plan-build work and storage evidence.
    #[must_use]
    pub const fn work_profile(&self) -> PlanWorkProfile {
        self.work_profile
    }

    /// Returns the explicit control representation used by this plan.
    #[must_use]
    pub const fn control_model(&self) -> ControlModel {
        ControlModel::FlatIterative
    }

    pub(crate) fn task_location(&self, id: TaskId) -> Option<(usize, usize)> {
        let task_index = *self.task_index.get(&id)?;
        Some((
            *self.batch_of.get(task_index)?,
            *self.position_in_batch.get(task_index)?,
        ))
    }
}

/// Builder for one immutable deterministic plan.
#[derive(Debug)]
pub struct PlanBuilder<T> {
    budget: Budget,
    tasks: Vec<TaskSpec<T>>,
    dependencies: Vec<(TaskId, TaskId)>,
}

impl<T> PlanBuilder<T> {
    /// Creates an empty builder under a positive batch budget.
    #[must_use]
    pub const fn new(budget: Budget) -> Self {
        Self {
            budget,
            tasks: Vec::new(),
            dependencies: Vec::new(),
        }
    }

    /// Replaces the task input snapshot.
    #[must_use]
    pub fn tasks(mut self, tasks: impl IntoIterator<Item = TaskSpec<T>>) -> Self {
        self.tasks = tasks.into_iter().collect();
        self
    }

    /// Replaces the dependency input snapshot.
    #[must_use]
    pub fn dependencies(
        mut self,
        dependencies: impl IntoIterator<Item = (TaskId, TaskId)>,
    ) -> Self {
        self.dependencies = dependencies.into_iter().collect();
        self
    }

    /// Validates input and constructs the unique deterministic first-fit plan.
    ///
    /// # Errors
    ///
    /// Returns a structured error for duplicate task identifiers, malformed
    /// graph input, cycles, resource exhaustion, or a checked internal domain
    /// overflow. Rejection publishes no plan or callback.
    pub fn build(self) -> Result<Plan<T>, PlanError> {
        let mut task_index = index_tasks(&self.tasks)?;
        let dependency_graph = CsrGraph::from_edges(
            self.tasks.iter().map(TaskSpec::id),
            self.dependencies.iter().copied(),
        )?;
        let topology = canonical_topology(&dependency_graph)?;
        let tasks = order_tasks_by_graph(self.tasks, dependency_graph.nodes(), &task_index)?;
        reindex_tasks(&mut task_index, dependency_graph.nodes())?;
        validate_costs(&tasks, self.budget)?;
        let placement = place_tasks(self.budget, &tasks, &dependency_graph, &topology.order)?;
        let position_in_batch =
            index_batch_positions(&placement.batches, &task_index, tasks.len())?;
        let canonical_order = collect_canonical_order(&dependency_graph, &topology.order)?;
        let work_profile = plan_work_profile(
            &tasks,
            &dependency_graph,
            &topology,
            &placement.work,
            placement.batches.len(),
        );

        Ok(Plan {
            budget: self.budget,
            tasks,
            task_index,
            dependency_graph,
            canonical_order,
            batches: placement.batches,
            batch_of: placement.batch_of,
            position_in_batch,
            work_profile,
        })
    }
}

struct PlacementResult {
    batches: Vec<PlanBatch>,
    batch_of: Vec<usize>,
    work: PlacementWork,
}

fn reindex_tasks(
    task_index: &mut HashMap<TaskId, usize>,
    stable_order: &[TaskId],
) -> Result<(), PlanError> {
    for (dense_index, task_id) in stable_order.iter().enumerate() {
        let indexed = task_index
            .get_mut(task_id)
            .ok_or(PlanError::InternalInvariant(
                "canonical graph node is absent from the retained task index",
            ))?;
        *indexed = dense_index;
    }
    Ok(())
}

fn validate_costs<T>(tasks: &[TaskSpec<T>], budget: Budget) -> Result<(), PlanError> {
    for task in tasks {
        if task.cost.get() > budget.get() {
            return Err(PlanError::ResourceExhausted {
                task: task.id,
                cost: task.cost,
                budget,
            });
        }
    }
    Ok(())
}

fn place_tasks<T>(
    budget: Budget,
    tasks: &[TaskSpec<T>],
    dependency_graph: &CsrGraph<TaskId>,
    topology: &[DenseId],
) -> Result<PlacementResult, PlanError> {
    let mut machine = PlacementMachine::new(tasks.len(), budget)?;
    let mut batch_of = vec![usize::MAX; tasks.len()];
    for dense in topology {
        let dense_index = dense.get() as usize;
        let task = tasks.get(dense_index).ok_or(PlanError::InternalInvariant(
            "topology identifier lies outside the task snapshot",
        ))?;
        let floor = dependency_floor(dependency_graph, *dense, &batch_of)?;
        batch_of[dense_index] = machine.place(task, floor)?;
    }
    let (batches, work) = machine.finish()?;
    Ok(PlacementResult {
        batches,
        batch_of,
        work,
    })
}

fn dependency_floor(
    dependency_graph: &CsrGraph<TaskId>,
    task: DenseId,
    batch_of: &[usize],
) -> Result<usize, PlanError> {
    let predecessors = dependency_graph
        .predecessors(task)?
        .ok_or(PlanError::InternalInvariant(
            "canonical dependency graph has no reverse CSR",
        ))?;
    let mut floor = 0usize;
    for predecessor in predecessors {
        let assigned = batch_of.get(predecessor.get() as usize).copied().ok_or(
            PlanError::InternalInvariant("predecessor lies outside the task snapshot"),
        )?;
        if assigned == usize::MAX {
            return Err(PlanError::InternalInvariant(
                "topological predecessor has not been assigned",
            ));
        }
        floor = floor.max(assigned.saturating_add(1));
    }
    Ok(floor)
}

fn index_batch_positions(
    batches: &[PlanBatch],
    task_index: &HashMap<TaskId, usize>,
    task_count: usize,
) -> Result<Vec<usize>, PlanError> {
    let mut positions = vec![usize::MAX; task_count];
    for batch in batches {
        for (position, task_id) in batch.task_ids.iter().enumerate() {
            let dense_index =
                task_index
                    .get(task_id)
                    .copied()
                    .ok_or(PlanError::InternalInvariant(
                        "planned task is absent from the task index",
                    ))?;
            let slot = positions
                .get_mut(dense_index)
                .ok_or(PlanError::InternalInvariant(
                    "planned task lies outside position storage",
                ))?;
            *slot = position;
        }
    }
    if positions.contains(&usize::MAX) {
        return Err(PlanError::InternalInvariant(
            "accepted plan omitted a task position",
        ));
    }
    Ok(positions)
}

fn collect_canonical_order(
    dependency_graph: &CsrGraph<TaskId>,
    topology: &[DenseId],
) -> Result<Vec<TaskId>, PlanError> {
    topology
        .iter()
        .map(|dense| dependency_graph.stable_id(*dense).copied())
        .collect::<Result<Vec<_>, _>>()
        .map_err(PlanError::from)
}

fn plan_work_profile<T>(
    tasks: &[TaskSpec<T>],
    dependency_graph: &CsrGraph<TaskId>,
    topology: &TopologyResult,
    placement: &PlacementWork,
    batch_count: usize,
) -> PlanWorkProfile {
    let vertices = tasks.len() as u64;
    let edges = dependency_graph.edge_count() as u64;
    let effect_slots = tasks.iter().fold(0u64, |total, task| {
        total
            .saturating_add(task.effects.reads().len() as u64)
            .saturating_add(task.effects.writes().len() as u64)
    });
    let auxiliary_slots_peak = 10u64
        .saturating_mul(vertices)
        .saturating_add(2u64.saturating_mul(edges))
        .saturating_add(effect_slots)
        .saturating_add(SEARCH_STACK_CAPACITY as u64);
    PlanWorkProfile {
        vertices,
        edges,
        topology_edge_visits: topology.edge_visits,
        ready_pushes: topology.ready_pushes,
        ready_pops: topology.ready_pops,
        batch_probes: placement.batch_probes,
        conflict_lookups: placement.conflict_lookups,
        capacity_tree_nodes: placement.capacity_tree_nodes,
        batch_sort_work_upper_bound: placement.batch_sort_work_upper_bound,
        allocation_count: 16u64.saturating_add(4u64.saturating_mul(batch_count as u64)),
        auxiliary_slots_peak,
    }
}

/// Atomic plan-construction failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// Two task inputs use the same stable identifier.
    DuplicateTaskId {
        /// Repeated identifier.
        task: TaskId,
    },
    /// At least one task participates in a dependency cycle.
    Cyclic,
    /// One task cannot fit even an otherwise empty batch.
    ResourceExhausted {
        /// Rejected task.
        task: TaskId,
        /// Task cost.
        cost: Cost,
        /// Configured budget.
        budget: Budget,
    },
    /// libvgraph rejected malformed or unrepresentable graph input.
    Graph(GraphError),
    /// Checked internal representation invariant failed.
    InternalInvariant(&'static str),
}

impl fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateTaskId { task } => write!(formatter, "duplicate task identifier {task}"),
            Self::Cyclic => formatter.write_str("dependency graph contains a cycle"),
            Self::ResourceExhausted { task, cost, budget } => write!(
                formatter,
                "task {task} cost {} exceeds batch budget {}",
                cost.get(),
                budget.get()
            ),
            Self::Graph(error) => error.fmt(formatter),
            Self::InternalInvariant(reason) => {
                write!(
                    formatter,
                    "scheduler representation invariant failed: {reason}"
                )
            }
        }
    }
}

impl std::error::Error for PlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Graph(error) => Some(error),
            _ => None,
        }
    }
}

impl From<GraphError> for PlanError {
    fn from(error: GraphError) -> Self {
        Self::Graph(error)
    }
}

#[derive(Debug, Clone, Copy)]
struct TopologyWork {
    edge_visits: u64,
    ready_pushes: u64,
    ready_pops: u64,
}

struct TopologyResult {
    order: Vec<DenseId>,
    edge_visits: u64,
    ready_pushes: u64,
    ready_pops: u64,
}

fn canonical_topology(graph: &CsrGraph<TaskId>) -> Result<TopologyResult, PlanError> {
    let vertex_count = graph.vertex_count();
    let reverse_offsets = graph.reverse_offsets().ok_or(PlanError::InternalInvariant(
        "canonical dependency graph has no reverse CSR",
    ))?;
    let mut indegrees = Vec::with_capacity(vertex_count);
    let mut ready = BinaryHeap::with_capacity(vertex_count);
    let mut work = TopologyWork {
        edge_visits: 0,
        ready_pushes: 0,
        ready_pops: 0,
    };
    for dense_index in 0..vertex_count {
        let indegree = reverse_offsets[dense_index + 1] - reverse_offsets[dense_index];
        indegrees.push(indegree);
        if indegree == 0 {
            let dense = u32::try_from(dense_index).map_err(|_| {
                PlanError::InternalInvariant("task domain does not fit a dense identifier")
            })?;
            ready.push(Reverse(DenseId::from_raw(dense)));
            work.ready_pushes += 1;
        }
    }

    let mut order = Vec::with_capacity(vertex_count);
    while let Some(Reverse(task)) = ready.pop() {
        work.ready_pops += 1;
        order.push(task);
        for successor in graph.successors(task)? {
            work.edge_visits += 1;
            let successor_index = successor.get() as usize;
            let indegree =
                indegrees
                    .get_mut(successor_index)
                    .ok_or(PlanError::InternalInvariant(
                        "successor lies outside the indegree domain",
                    ))?;
            *indegree = indegree
                .checked_sub(1)
                .ok_or(PlanError::InternalInvariant("successor indegree underflow"))?;
            if *indegree == 0 {
                ready.push(Reverse(*successor));
                work.ready_pushes += 1;
            }
        }
    }
    if order.len() != vertex_count {
        return Err(PlanError::Cyclic);
    }
    Ok(TopologyResult {
        order,
        edge_visits: work.edge_visits,
        ready_pushes: work.ready_pushes,
        ready_pops: work.ready_pops,
    })
}

fn index_tasks<T>(tasks: &[TaskSpec<T>]) -> Result<HashMap<TaskId, usize>, PlanError> {
    let mut task_index = HashMap::with_capacity(tasks.len());
    for (index, task) in tasks.iter().enumerate() {
        if task_index.insert(task.id, index).is_some() {
            return Err(PlanError::DuplicateTaskId { task: task.id });
        }
    }
    Ok(task_index)
}

fn order_tasks_by_graph<T>(
    tasks: Vec<TaskSpec<T>>,
    stable_order: &[TaskId],
    task_index: &HashMap<TaskId, usize>,
) -> Result<Vec<TaskSpec<T>>, PlanError> {
    if tasks.len() != stable_order.len() || tasks.len() != task_index.len() {
        return Err(PlanError::InternalInvariant(
            "task snapshot and canonical graph domains differ",
        ));
    }
    let mut slots: Vec<Option<TaskSpec<T>>> = tasks.into_iter().map(Some).collect();
    let mut canonical = Vec::with_capacity(slots.len());
    for task_id in stable_order {
        let index = task_index
            .get(task_id)
            .copied()
            .ok_or(PlanError::InternalInvariant(
                "canonical graph node is absent from the task index",
            ))?;
        let task =
            slots
                .get_mut(index)
                .and_then(Option::take)
                .ok_or(PlanError::InternalInvariant(
                    "canonical task slot was moved more than once",
                ))?;
        canonical.push(task);
    }
    Ok(canonical)
}

struct BatchState {
    task_ids: Vec<TaskId>,
    total_cost: u64,
    reads: HashSet<ResourceId>,
    writes: HashSet<ResourceId>,
}

impl BatchState {
    fn new(task: &TaskSpec<impl Sized>) -> Self {
        let mut state = Self {
            task_ids: Vec::with_capacity(1),
            total_cost: 0,
            reads: HashSet::with_capacity(task.effects.reads().len()),
            writes: HashSet::with_capacity(task.effects.writes().len()),
        };
        state.insert(task);
        state
    }

    fn conflicts(&self, effects: &TaskEffects, lookups: &mut u64) -> bool {
        for resource in effects.writes() {
            *lookups += 1;
            if self.writes.contains(resource) {
                return true;
            }
            *lookups += 1;
            if self.reads.contains(resource) {
                return true;
            }
        }
        for resource in effects.reads() {
            *lookups += 1;
            if self.writes.contains(resource) {
                return true;
            }
        }
        false
    }

    fn insert<T>(&mut self, task: &TaskSpec<T>) {
        self.task_ids.push(task.id);
        self.total_cost += task.cost.get();
        self.reads.reserve(task.effects.reads().len());
        self.writes.reserve(task.effects.writes().len());
        self.reads.extend(task.effects.reads().iter().copied());
        self.writes.extend(task.effects.writes().iter().copied());
    }
}

#[derive(Default)]
struct PlacementWork {
    batch_probes: u64,
    conflict_lookups: u64,
    capacity_tree_nodes: u64,
    batch_sort_work_upper_bound: u64,
}

struct PlacementMachine {
    budget: Budget,
    batches: Vec<BatchState>,
    capacities: CapacityIndex,
    work: PlacementWork,
}

impl PlacementMachine {
    fn new(max_batches: usize, budget: Budget) -> Result<Self, PlanError> {
        Ok(Self {
            budget,
            batches: Vec::with_capacity(max_batches),
            capacities: CapacityIndex::new(max_batches)?,
            work: PlacementWork::default(),
        })
    }

    fn place<T>(&mut self, task: &TaskSpec<T>, floor: usize) -> Result<usize, PlanError> {
        let mut search_start = floor;
        while let Some(candidate) = self
            .capacities
            .first_with_capacity(search_start, task.cost.get())
        {
            self.work.batch_probes += 1;
            let batch = self
                .batches
                .get(candidate)
                .ok_or(PlanError::InternalInvariant(
                    "capacity index returned an absent batch",
                ))?;
            if !batch.conflicts(&task.effects, &mut self.work.conflict_lookups) {
                let batch = self
                    .batches
                    .get_mut(candidate)
                    .ok_or(PlanError::InternalInvariant(
                        "selected batch disappeared before insertion",
                    ))?;
                batch.insert(task);
                let remaining = self.budget.get().checked_sub(batch.total_cost).ok_or(
                    PlanError::InternalInvariant(
                        "batch cost exceeded budget after capacity admission",
                    ),
                )?;
                self.capacities.update(candidate, remaining)?;
                return Ok(candidate);
            }
            search_start = candidate.saturating_add(1);
        }

        let batch_index = self.batches.len();
        self.batches.push(BatchState::new(task));
        self.capacities
            .append(self.budget.get() - task.cost.get())?;
        Ok(batch_index)
    }

    fn finish(mut self) -> Result<(Vec<PlanBatch>, PlacementWork), PlanError> {
        self.work.capacity_tree_nodes = self.capacities.nodes_visited;
        let mut batches = Vec::with_capacity(self.batches.len());
        for state in self.batches {
            let member_count = state.task_ids.len() as u64;
            let levels = if member_count <= 1 {
                0
            } else {
                u64::from(u64::BITS - (member_count - 1).leading_zeros())
            };
            let sort_bound = if member_count <= 1 {
                0
            } else {
                2u64.saturating_mul(member_count)
                    .saturating_mul(levels.saturating_add(1))
            };
            self.work.batch_sort_work_upper_bound = self
                .work
                .batch_sort_work_upper_bound
                .saturating_add(sort_bound);
            let task_ids = BinaryHeap::from(state.task_ids).into_sorted_vec();
            let total_cost = Cost::new(state.total_cost).ok_or(PlanError::InternalInvariant(
                "nonempty batch has zero total cost",
            ))?;
            batches.push(PlanBatch {
                task_ids,
                total_cost,
            });
        }
        Ok((batches, self.work))
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct SearchFrame {
    node: usize,
    left: usize,
    right: usize,
}

struct CapacityIndex {
    leaf_count: usize,
    batch_count: usize,
    maxima: Vec<u64>,
    search_stack: Vec<SearchFrame>,
    nodes_visited: u64,
}

impl CapacityIndex {
    fn new(max_batches: usize) -> Result<Self, PlanError> {
        let leaf_count =
            max_batches
                .max(1)
                .checked_next_power_of_two()
                .ok_or(PlanError::InternalInvariant(
                    "capacity-tree leaf count overflow",
                ))?;
        let slot_count = leaf_count
            .checked_mul(2)
            .ok_or(PlanError::InternalInvariant(
                "capacity-tree storage overflow",
            ))?;
        Ok(Self {
            leaf_count,
            batch_count: 0,
            maxima: vec![0; slot_count],
            search_stack: Vec::with_capacity(SEARCH_STACK_CAPACITY),
            nodes_visited: 0,
        })
    }

    fn append(&mut self, remaining: u64) -> Result<(), PlanError> {
        if self.batch_count >= self.leaf_count {
            return Err(PlanError::InternalInvariant(
                "capacity tree cannot represent another batch",
            ));
        }
        let index = self.batch_count;
        self.batch_count += 1;
        self.update(index, remaining)
    }

    fn update(&mut self, index: usize, remaining: u64) -> Result<(), PlanError> {
        if index >= self.batch_count {
            return Err(PlanError::InternalInvariant(
                "capacity update references an absent batch",
            ));
        }
        let mut node = self.leaf_count + index;
        let leaf = self
            .maxima
            .get_mut(node)
            .ok_or(PlanError::InternalInvariant(
                "capacity leaf lies outside tree storage",
            ))?;
        *leaf = remaining;
        while node > 1 {
            node /= 2;
            let left = self.maxima[node * 2];
            let right = self.maxima[node * 2 + 1];
            self.maxima[node] = left.max(right);
        }
        Ok(())
    }

    fn first_with_capacity(&mut self, start: usize, required: u64) -> Option<usize> {
        if start >= self.batch_count {
            return None;
        }
        self.search_stack.clear();
        self.search_stack.push(SearchFrame {
            node: 1,
            left: 0,
            right: self.leaf_count,
        });
        while let Some(frame) = self.search_stack.pop() {
            self.nodes_visited += 1;
            if frame.right <= start
                || frame.left >= self.batch_count
                || self.maxima[frame.node] < required
            {
                continue;
            }
            if frame.right - frame.left == 1 {
                return Some(frame.left);
            }
            let middle = frame.left + (frame.right - frame.left) / 2;
            self.search_stack.push(SearchFrame {
                node: frame.node * 2 + 1,
                left: middle,
                right: frame.right,
            });
            self.search_stack.push(SearchFrame {
                node: frame.node * 2,
                left: frame.left,
                right: middle,
            });
        }
        None
    }
}
