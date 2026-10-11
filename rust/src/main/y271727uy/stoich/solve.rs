//! General compressed realization of a recipe graph.
//!
//! A block is the pair `(required, net)` of a firing sequence. Sequencing and repetition are
//! closed forms, identical to ECO's `CircuitSummary` and thunderbolt's verifier block:
//!
//! ```text
//! required(A;B) = max(required(A), required(B) - net(A))
//! net(A;B)      = net(A) + net(B)
//! required(A^q) = required(A) + (q-1)·max(-net(A), 0)
//! ```
//!
//! Search nodes are blocks, not individual firings. A macro-step is one recipe fired for its
//! maximum safe batch, and a cycle whose marking returns is repeated instead of unrolled.

use crate::balance::{self, Balance};
use crate::bounds::{self, Bounds, FiringInterval};
use crate::quantity::{SignedAmount, UnsignedAmount};

/// Hard cap on expanded markings. A caller may ask for fewer, never for more.
const MAX_STATES: u64 = 250_000;
use num_traits::ToPrimitive;
use crate::shape::{self, ShapeMiss};
use std::collections::{BTreeSet, BinaryHeap, HashMap};
use std::rc::Rc;

pub const MAX_DEPTH: u32 = 4_096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SolveRequest {
    pub consumed: Vec<Vec<i64>>,
    pub produced: Vec<Vec<i64>>,
    pub stock: Vec<i64>,
    pub target: Vec<i64>,
    /// Items whose deficit is booked as an external import rather than a seed.
    pub importable: Vec<bool>,
    /// When set, realize this firing vector instead of searching for one.
    pub candidate: Option<Vec<i64>>,
    /// Fixed cycle order, each entry `(recipe, count within one cycle)`.
    pub order: Option<Vec<(usize, i64)>>,
    pub target_item: Option<usize>,
    pub max_states: u64,
    /// Skip the state-equation witness and search markings directly. The equation answers a mutually
    /// growing pair before any search, so a caller measuring the search budget sets this.
    pub skip_equation: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Success,
    /// The witness is exact, but the reported seed is not covered by stock.
    InsufficientSeed,
    /// Material balance has no nonnegative solution.
    Infeasible,
    /// The state budget closed before reachability was proved.
    BudgetExhausted,
    /// Counts or seeds do not fit back into a signed 64-bit result.
    Unrepresentable,
    /// Intervals remain, and no executable block was found within the budget.
    NotClosed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Step {
    pub recipe: usize,
    pub count: UnsignedAmount,
    /// Width of the cycle ending at this step. Zero when the step is not a repetition marker.
    pub repeat_width: u32,
    pub repetitions: UnsignedAmount,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SolveResult {
    pub status: Status,
    pub firings: Vec<UnsignedAmount>,
    pub seed: Vec<SignedAmount>,
    pub external: Vec<SignedAmount>,
    pub shortfall: Vec<SignedAmount>,
    pub steps: Vec<Step>,
    pub intervals: Vec<FiringInterval>,
    pub states: u64,
    /// Pareto frontier of the internal input each recipe is short of before its first firing. Only
    /// attached to a proven shortage; empty otherwise. A candidate dominates another when it needs no
    /// more of anything and is compared per item, never as a mixed total.
    pub startup: Vec<Vec<SignedAmount>>,
}

pub fn solve(request: &SolveRequest) -> Result<SolveResult, &'static str> {
    validate(request)?;
    let recipes = request.consumed.len();
    let items = request.stock.len();
    let net = net_columns(request);

    let bounds = bounds::propagate(&net, &request.stock, &request.target, &request.importable, request.candidate.as_deref())?;
    let Bounds::Intervals(intervals) = bounds else {
        return Ok(empty(Status::Infeasible, recipes, items));
    };
    if intervals.iter().any(|interval| !interval.lower.fits_i64() || interval.upper.as_ref().is_some_and(|u| !u.fits_i64())) {
        return Ok(with_intervals(Status::Unrepresentable, recipes, items, intervals));
    }

    if request.candidate.is_none() && request.order.is_none() {
        if let Some(result) = try_single_growth(request, &intervals)? {
            return Ok(result);
        }
    }
    if let Some(order) = &request.order {
        return realize_fixed_order(request, order, &intervals);
    }

    let forced = if intervals.iter().all(FiringInterval::is_fixed) {
        Some(intervals.iter().map(|interval| interval.lower.to_i64().unwrap_or(0)).collect::<Vec<_>>())
    } else {
        request.candidate.clone()
    };

    if let Some(forced) = forced.as_deref() {
        return Search::new(request, Some(forced), &intervals).run();
    }
    // ECO solves the state equation before searching markings. The minimum integer vector is replayed
    // in recipe order, repeating a lap while its seed is covered. A vector that cannot be ordered, or a
    // balance the branch budget did not settle, falls through to the exhaustive marking search.
    if !request.skip_equation {
        if let Some(vector) = minimum_vector(request, &intervals) {
            if let Some(result) = replay_vector(request, &vector, &intervals) {
                return Ok(result);
            }
        }
    }
    Search::new(request, None, &intervals).run()
}

fn minimum_vector(request: &SolveRequest, intervals: &[FiringInterval]) -> Option<Vec<i64>> {
    let net = net_columns(request);
    let balance = balance::minimize(&net, &request.stock, &request.target, &request.importable);
    let counts = match balance {
        Balance::Optimal(counts) | Balance::Feasible(counts) => counts,
        Balance::Infeasible | Balance::Unknown => return None,
    };
    let counts: Option<Vec<i64>> = counts.iter().map(|count| count.to_i64()).collect();
    // A vector outside the propagated intervals is not a firing count this stock can reach. The open
    // growth ray is the usual case: the equation is unbounded, so its witness must not skip the search.
    counts.filter(|counts| counts.iter().zip(intervals).all(|(count, interval)| interval.contains(*count)))
}

fn try_single_growth(request: &SolveRequest, intervals: &[FiringInterval]) -> Result<Option<SolveResult>, &'static str> {
    if request.consumed.len() != 1 {
        return Ok(None);
    }
    match shape::single_growth(&request.consumed[0], &request.produced[0], &request.stock, &request.target) {
        Ok(shape) => {
            let count = shape.firings[0].clone();
            Ok(Some(from_shape(request, shape, intervals, vec![Step {
                recipe: 0,
                count,
                repeat_width: 0,
                repetitions: UnsignedAmount::one(),
            }])?))
        }
        Err(ShapeMiss::NotApplicable) => Ok(None),
        Err(ShapeMiss::Unrepresentable) => Ok(Some(with_intervals(Status::Unrepresentable, 1, request.stock.len(), intervals.to_vec()))),
    }
}

fn from_shape(
    request: &SolveRequest,
    shape: shape::Shape,
    intervals: &[FiringInterval],
    steps: Vec<Step>,
) -> Result<SolveResult, &'static str> {
    Ok(account(request, shape.firings, shape.seed, steps, intervals.to_vec(), 0))
}

fn realize_fixed_order(
    request: &SolveRequest,
    order: &[(usize, i64)],
    intervals: &[FiringInterval],
) -> Result<SolveResult, &'static str> {
    let Some(target_item) = request.target_item else {
        return Err("a fixed order needs its target item");
    };
    let target_amount = *request.target.get(target_item).ok_or("target item is outside the graph")?;
    match shape::fixed_order(&request.consumed, &request.produced, order, &request.stock, target_item, target_amount) {
        Ok(shape) => {
            let steps = order
                .iter()
                .map(|&(recipe, count)| Step {
                    recipe,
                    count: UnsignedAmount::Small(count),
                    repeat_width: 0,
                    repetitions: UnsignedAmount::one(),
                })
                .collect();
            Ok(account(request, shape.firings, shape.seed, steps, intervals.to_vec(), 0))
        }
        Err(ShapeMiss::NotApplicable) => Ok(with_intervals(Status::NotClosed, request.consumed.len(), request.stock.len(), intervals.to_vec())),
        Err(ShapeMiss::Unrepresentable) => {
            Ok(with_intervals(Status::Unrepresentable, request.consumed.len(), request.stock.len(), intervals.to_vec()))
        }
    }
}

fn account(
    request: &SolveRequest,
    firings: Vec<UnsignedAmount>,
    seed: Vec<SignedAmount>,
    steps: Vec<Step>,
    intervals: Vec<FiringInterval>,
    states: u64,
) -> SolveResult {
    let items = request.stock.len();
    let mut external = vec![SignedAmount::zero(); items];
    let mut retained = seed.clone();
    let mut shortfall = vec![SignedAmount::zero(); items];
    for item in 0..items {
        let have = SignedAmount::from_i64(request.stock[item]);
        let missing = retained[item].checked_sub(&have);
        if missing.signum() > 0 {
            shortfall[item] = missing.clone();
            if request.importable[item] {
                external[item] = missing;
                retained[item] = SignedAmount::zero();
                shortfall[item] = SignedAmount::zero();
            }
        }
    }
    let representable = firings.iter().all(UnsignedAmount::fits_i64)
        && retained.iter().chain(external.iter()).chain(shortfall.iter()).all(SignedAmount::fits_i64)
        && steps.iter().all(|step| step.count.fits_i64() && step.repetitions.fits_i64());
    let covered = shortfall.iter().all(SignedAmount::is_zero);
    let status = if !representable {
        Status::Unrepresentable
    } else if covered {
        Status::Success
    } else {
        Status::InsufficientSeed
    };
    let startup = if status == Status::InsufficientSeed { startup_candidates(request) } else { Vec::new() };
    SolveResult { status, firings, seed: retained, external, shortfall, steps, intervals, states, startup }
}

struct Recipe {
    consumed: Vec<(usize, i64)>,
    produced: Vec<(usize, i64)>,
    net: Vec<(usize, i64)>,
}

struct Search<'a> {
    request: &'a SolveRequest,
    recipes: Vec<Recipe>,
    remaining_cap: Option<Vec<i64>>,
    intervals: &'a [FiringInterval],
}

impl<'a> Search<'a> {
    fn new(request: &'a SolveRequest, forced: Option<&[i64]>, intervals: &'a [FiringInterval]) -> Self {
        let recipes = (0..request.consumed.len())
            .map(|recipe| Recipe {
                consumed: sparse(&request.consumed[recipe]),
                produced: sparse(&request.produced[recipe]),
                net: sparse_net(&request.consumed[recipe], &request.produced[recipe]),
            })
            .collect();
        Self { request, recipes, remaining_cap: forced.map(ToOwned::to_owned), intervals }
    }

    fn run(&mut self) -> Result<SolveResult, &'static str> {
        let items = self.request.stock.len();
        let recipes = self.recipes.len();
            let limit = self.request.max_states.min(MAX_STATES).max(1);
        // Best-first, like ECO: the queue orders markings by remaining deficit, and a state is
        // stored before it is expanded. The queue emptying is the exhaustive-closure proof.
        let mut queue = BinaryHeap::new();
        let root_block = Rc::new(Block::empty(items, recipes));
        let root = Node {
            marking: self.request.stock.clone(),
            remaining: self.remaining_cap.clone().map(|counts| counts.into_iter().map(Some).collect()).unwrap_or_else(|| vec![None; recipes]),
            block: Rc::clone(&root_block),
            trace: Rc::new(Trace::Root),
            depth: 0,
            signature: signature_of(&self.request.stock),
            rank: deficit(&self.request.stock, &self.request.target),
            ordinal: 0,
        };
        if satisfied(&root.marking, &self.request.target) && root.remaining.iter().all(|count| count.is_none_or(|count| count == 0)) {
            return Ok(account(self.request, vec![UnsignedAmount::zero(); recipes], vec![SignedAmount::zero(); items], Vec::new(), self.intervals.to_vec(), 1));
        }
        let mut seen = HashMap::new();
        seen.insert(root.signature, 0u64);
        queue.push(root);
        let mut states = 1u64;
        let mut budget_hit = false;
        let mut ordinal = 0u64;

        while let Some(node) = queue.pop() {
            if node.depth >= MAX_DEPTH {
                budget_hit = true;
                continue;
            }
            for recipe in 0..recipes {
                if node.remaining[recipe] == Some(0) {
                    continue;
                }
                let cap = node.remaining[recipe].unwrap_or_else(|| {
                    // No caller cap. A growing recipe only needs enough firings to close the
                    // outstanding targets; anything past that revisits a larger marking.
                    outstanding_firings(&self.recipes, recipe, &node.marking, &self.request.target)
                });
                let safe = safe_batch(&self.recipes[recipe], &node.marking, cap, &self.request.importable);
                if safe <= 0 {
                    continue;
                }
                // ECO keeps the single firing, the counts that unblock another recipe, and the
                // maximum safe batch. Dropping the smaller ones would skip a valid interleaving.
                for batch in candidates(&self.recipes, recipe, &node.marking, safe) {
                    let mut next_marking = node.marking.clone();
                    apply_batch(&self.recipes[recipe], &mut next_marking, batch, &self.request.importable);
                    // A marking already queued needs nothing else from this batch. Checking before the
                    // remaining-vector clone skips that allocation for every repeated state.
                    let signature = signature_of(&next_marking);
                    if seen.contains_key(&signature) {
                        continue;
                    }
                    let mut next_remaining = node.remaining.clone();
                    if let Some(left) = next_remaining[recipe].as_mut() {
                        *left -= batch;
                    }
                    let fired = Rc::new(node.block.fire(recipe, batch, &self.recipes[recipe], items));
                    let trace = Rc::new(Trace::Step { previous: Rc::clone(&node.trace), recipe, count: batch });
                    let (fired, trace, next_marking, next_remaining) = fold_cycle(
                        fired,
                        trace,
                        &next_marking,
                        &next_remaining,
                        &self.recipes,
                        &self.request.importable,
                    );
                    if satisfied(&next_marking, &self.request.target)
                        && next_remaining.iter().all(|count| count.is_none_or(|count| count == 0))
                    {
                        return Ok(account(
                            self.request,
                            fired.counts.clone(),
                            fired.required.clone(),
                            steps_of(&trace),
                            self.intervals.to_vec(),
                            states,
                        ));
                    }
                    if seen.contains_key(&signature) {
                        continue;
                    }
                    if states >= limit {
                        budget_hit = true;
                        break;
                    }
                    states += 1;
                    ordinal += 1;
                    seen.insert(signature, states);
                    queue.push(Node {
                        rank: deficit(&next_marking, &self.request.target),
                        ordinal,
                        signature,
                        marking: next_marking,
                        remaining: next_remaining,
                        block: fired,
                        trace,
                        depth: node.depth + 1,
                    });
                }
                if budget_hit {
                    break;
                }
            }
            if budget_hit {
                break;
            }
        }

        // Empty queue: every reachable marking was expanded and none met the target. That is ECO's
        // exhaustive closure, so the stock is proven short. A budget cut proves nothing.
        let status = if budget_hit { Status::BudgetExhausted } else { Status::InsufficientSeed };
        Ok(with_intervals(status, recipes, items, self.intervals.to_vec()))
    }
}

#[derive(Clone)]
struct Node {
    /// Lower deficit comes out of the heap first. The ordinal breaks ties toward newer nodes.
    rank: u128,
    ordinal: u64,
    marking: Vec<i64>,
    /// `None` means the caller did not cap this recipe.
    remaining: Vec<Option<i64>>,
    block: Rc<Block>,
    trace: Rc<Trace>,
    depth: u32,
    signature: u64,
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.rank == other.rank && self.ordinal == other.ordinal
    }
}

impl Eq for Node {}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // BinaryHeap pops the greatest element, so a smaller deficit has to compare as greater.
        other.rank.cmp(&self.rank).then(self.ordinal.cmp(&other.ordinal))
    }
}

/// Shared firing history. Cloning a node copies the pointer, not the path.
enum Trace {
    Root,
    Step { previous: Rc<Trace>, recipe: usize, count: i64 },
    /// `body` is the first execution; it is then executed `extra` more times.
    Repeat { previous: Rc<Trace>, body: Rc<Trace>, width: u32, extra: i64 },
}

fn steps_of(trace: &Trace) -> Vec<Step> {
    fn walk(trace: &Trace, out: &mut Vec<Step>) {
        match trace {
            Trace::Root => {}
            Trace::Step { previous, recipe, count } => {
                walk(previous, out);
                out.push(Step { recipe: *recipe, count: UnsignedAmount::Small(*count), repeat_width: 0, repetitions: UnsignedAmount::one() });
            }
            Trace::Repeat { previous, body, width, extra } => {
                walk(previous, out);
                let start = out.len();
                walk(body, out);
                let width = (*width).max((out.len() - start) as u32);
                if let Some(last) = out.last_mut() {
                    last.repeat_width = width;
                    last.repetitions = UnsignedAmount::Small(extra + 1);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(trace, &mut out);
    out
}

#[derive(Clone)]
struct Block {
    required: Vec<SignedAmount>,
    net: Vec<SignedAmount>,
    counts: Vec<UnsignedAmount>,
}

impl Block {
    fn empty(items: usize, recipes: usize) -> Self {
        Self {
            required: vec![SignedAmount::zero(); items],
            net: vec![SignedAmount::zero(); items],
            counts: vec![UnsignedAmount::zero(); recipes],
        }
    }

    fn fire(&self, recipe: usize, count: i64, recipe_data: &Recipe, items: usize) -> Self {
        let mut one = Self::empty(items, self.counts.len());
        // The net list is sparse and unordered, so index it once instead of scanning it per input.
        let mut net_of = vec![0i64; items];
        for &(item, delta) in &recipe_data.net {
            net_of[item] = delta;
        }
        for &(item, amount) in &recipe_data.consumed {
            let delta = net_of[item];
            let mut needed = SignedAmount::from_i64(amount);
            if delta < 0 {
                needed = needed.checked_add(&SignedAmount::from_i64(-delta).checked_mul_i64(count - 1));
            }
            one.required[item] = needed;
        }
        for &(item, delta) in &recipe_data.net {
            one.net[item] = SignedAmount::from_i64(delta).checked_mul_i64(count);
        }
        one.counts[recipe] = UnsignedAmount::Small(count);
        self.then(&one)
    }

    fn then(&self, next: &Self) -> Self {
        let mut required = self.required.clone();
        let mut net = self.net.clone();
        let mut counts = self.counts.clone();
        for item in 0..required.len() {
            let shifted = next.required[item].checked_sub(&self.net[item]);
            if shifted > required[item] {
                required[item] = shifted;
            }
            net[item] = net[item].checked_add(&next.net[item]);
        }
        for recipe in 0..counts.len() {
            counts[recipe] = counts[recipe].checked_add(&next.counts[recipe]);
        }
        Self { required, net, counts }
    }

    /// `extra` additional executions beyond the one already represented by this block.
    fn repeated(&self, extra: i64) -> Self {
        let times = UnsignedAmount::Small(extra + 1);
        let mut required = self.required.clone();
        let mut net = self.net.clone();
        let mut counts = self.counts.clone();
        for item in 0..self.net.len() {
            if self.net[item].signum() < 0 {
                let slope = SignedAmount::zero().checked_sub(&self.net[item]);
                required[item] = required[item].checked_add(&slope.checked_mul_i64(extra));
            }
            net[item] = self.net[item].checked_mul(&times);
        }
        let factor = UnsignedAmount::Small(extra + 1);
        for count in &mut counts {
            *count = count.checked_mul(&factor);
        }
        Self { required, net, counts }
    }
}

/// Repeat the longest suffix whose own prefix requirement is already covered and whose remaining
/// firings are an integer multiple of the suffix. The repetition formula is the block rule, so the
/// cost does not depend on the repetition count.
/// Replay one integer firing vector the way ECO's `equationWitness` does.
///
/// Each lap fires every recipe for its current safe batch, then repeats that lap while the repeated
/// seed stays covered and the vector still owes those firings. A lap that fires nothing raises the
/// least-missing recipe's internal inputs up to one firing, which is a proposal the seed reports.
fn replay_vector(request: &SolveRequest, vector: &[i64], intervals: &[FiringInterval]) -> Option<SolveResult> {
    let recipes: Vec<Recipe> = (0..request.consumed.len())
        .map(|recipe| Recipe {
            consumed: sparse(&request.consumed[recipe]),
            produced: sparse(&request.produced[recipe]),
            net: sparse_net(&request.consumed[recipe], &request.produced[recipe]),
        })
        .collect();
    let mut remaining = vector.to_vec();
    let mut marking = request.stock.clone();
    let mut trace = Rc::new(Trace::Root);
    let mut block = Rc::new(Block::empty(request.stock.len(), recipes.len()));
    let mut guard = 0u32;
    while remaining.iter().any(|count| *count > 0) {
        guard += 1;
        if guard > 64 {
            return None;
        }
        let lap_start = steps_of(&trace).len();
        for recipe in 0..recipes.len() {
            if remaining[recipe] <= 0 {
                continue;
            }
            let safe = safe_batch(&recipes[recipe], &marking, remaining[recipe], &request.importable);
            if safe <= 0 {
                continue;
            }
            apply_batch(&recipes[recipe], &mut marking, safe, &request.importable);
            remaining[recipe] -= safe;
            block = Rc::new(block.fire(recipe, safe, &recipes[recipe], request.stock.len()));
            trace = Rc::new(Trace::Step { previous: trace, recipe, count: safe });
        }
        let steps = steps_of(&trace);
        if steps.len() == lap_start {
            // Nothing is enabled. Raise the internal inputs of the recipe that is least short, then
            // retry. The raised stock is a proposal: accounting reports it as a seed, not as owned.
            let Some(recipe) = least_missing(&remaining, &marking, request) else { return None };
            for (item, &needed) in request.consumed[recipe].iter().enumerate() {
                if !request.importable[item] {
                    marking[item] = marking[item].max(needed);
                }
            }
            continue;
        }
        let lap = block_of(&steps[lap_start..], &recipes, request.stock.len());
        let extra = remaining
            .iter()
            .zip(&lap.counts)
            .filter_map(|(left, count)| count.to_i64().filter(|count| *count > 0).map(|count| left / count))
            .min()
            .unwrap_or(0);
        let extra = repetitions_covered(&lap, &marking, &request.importable, extra);
        if extra > 0 {
            let repeated = lap.repeated(extra);
            for (item, delta) in repeated.net.iter().enumerate() {
                marking[item] = add_amount(marking[item], delta);
            }
            for (recipe, count) in lap.counts.iter().enumerate() {
                if let Some(count) = count.to_i64().filter(|count| *count > 0) {
                    remaining[recipe] -= count * extra;
                }
            }
            block = Rc::new(block.then(&repeated));
            trace = Rc::new(Trace::Repeat { previous: trace_before(&steps[..lap_start]), body: trace_from(&steps[lap_start..]), width: (steps.len() - lap_start) as u32, extra });
        }
    }
    if !satisfied(&marking, &request.target) {
        return None;
    }
    let counts: Vec<UnsignedAmount> = vector.iter().copied().map(UnsignedAmount::Small).collect();
    let seed = concurrent_seed(request, &block, &steps_of(&trace));
    Some(account(request, counts, seed, steps_of(&trace), intervals.to_vec(), 0))
}

/// Sequential replay holds one returned catalyst. A batch holds `count * input` of them at once, so a
/// catalyst the block consumes and returns in full is reserved at that concurrent amount, capped by
/// the stock that actually exists.
fn concurrent_seed(request: &SolveRequest, block: &Block, steps: &[Step]) -> Vec<SignedAmount> {
    let mut seed = block.required.clone();
    for step in steps {
        let Some(count) = step.count.to_i64() else { continue };
        for (item, &input) in request.consumed[step.recipe].iter().enumerate() {
            if request.importable[item] || input <= 0 {
                continue;
            }
            let delta = request.produced[step.recipe][item] - input;
            if delta != 0 || request.produced[step.recipe][item] < input {
                continue;
            }
            let held = count.saturating_mul(input).min(request.stock[item]);
            let concurrent = SignedAmount::from_i64(held);
            if concurrent > seed[item] {
                seed[item] = concurrent;
            }
        }
    }
    seed
}

/// The recipe still owed firings whose internal inputs are least short of one firing.
///
/// ECO scores each input by its deficit divided by the input, so a recipe missing half of a large
/// input ranks ahead of one missing all of a small one. The total is only a tie break.
fn least_missing(remaining: &[i64], marking: &[i64], request: &SolveRequest) -> Option<usize> {
    let mut best: Option<(usize, (i64, i64))> = None;
    for (recipe, &left) in remaining.iter().enumerate() {
        if left <= 0 {
            continue;
        }
        let mut numerator = 0i64;
        let mut denominator = 0i64;
        for (item, &needed) in request.consumed[recipe].iter().enumerate() {
            if request.importable[item] || needed <= 0 {
                continue;
            }
            let deficit = (needed - marking[item]).max(0);
            numerator += deficit;
            denominator += needed;
        }
        let score = (numerator, denominator);
        if best.as_ref().is_none_or(|(_, current)| better_deficit(score, *current)) {
            best = Some((recipe, score));
        }
    }
    best.map(|(recipe, _)| recipe)
}

fn better_deficit(candidate: (i64, i64), current: (i64, i64)) -> bool {
    // Compare deficit/input without mixing units: a/b < c/d iff a*d < c*b.
    candidate.0.saturating_mul(current.1) < current.0.saturating_mul(candidate.1)
}

fn block_of(steps: &[Step], recipes: &[Recipe], items: usize) -> Block {
    let mut block = Block::empty(items, recipes.len());
    for step in steps {
        let Some(count) = step.count.to_i64() else { continue };
        block = block.fire(step.recipe, count, &recipes[step.recipe], items);
    }
    block
}

fn repetitions_covered(lap: &Block, marking: &[i64], importable: &[bool], requested: i64) -> i64 {
    let mut covered = requested;
    for (item, required) in lap.required.iter().enumerate() {
        if importable[item] {
            continue;
        }
        if SignedAmount::from_i64(marking[item]) < *required {
            return 0;
        }
        if lap.net[item].signum() < 0 {
            let loss = lap.net[item].to_i64().map(|delta| -delta).unwrap_or(i64::MAX);
            let spare = marking[item] - required.to_i64().unwrap_or(i64::MAX);
            covered = covered.min(spare / loss + 1);
        }
    }
    covered.max(0)
}

fn fold_cycle(
    block: Rc<Block>,
    trace: Rc<Trace>,
    marking: &[i64],
    remaining: &[Option<i64>],
    recipes: &[Recipe],
    importable: &[bool],
) -> (Rc<Block>, Rc<Trace>, Vec<i64>, Vec<Option<i64>>) {
    let flat = steps_of(&trace);
    let mut best: Option<(usize, i64, Block)> = None;
    let mut suffix = Block::empty(marking.len(), remaining.len());
    for start in (0..flat.len()).rev() {
        let step = &flat[start];
        let Some(count) = step.count.to_i64() else { break };
        let one = Block::empty(marking.len(), remaining.len()).fire(step.recipe, count, &recipes[step.recipe], marking.len());
        suffix = one.then(&suffix);
        if suffix.counts.iter().all(|count| count.is_zero()) {
            continue;
        }
        let counted = |count: &UnsignedAmount| count.to_i64().filter(|count| *count > 0);
        let times = remaining
            .iter()
            .zip(&suffix.counts)
            .filter_map(|(left, count)| left.and_then(|left| counted(count).map(|count| left / count)))
            .min();
        let Some(times) = times.filter(|times| *times >= 2) else { continue };
        if remaining.iter().zip(&suffix.counts).any(|(left, count)| {
            counted(count).is_some_and(|count| left.is_none_or(|left| left / count != times))
        }) {
            continue;
        }
        if !suffix_covered(&suffix, marking, importable, times) {
            continue;
        }
        best = Some((start, times, suffix.clone()));
    }
    let Some((start, times, suffix)) = best else {
        return (block, trace, marking.to_vec(), remaining.to_vec());
    };
    let extra = times - 1;
    let repeated = suffix.repeated(extra);
    let mut next_marking = marking.to_vec();
    for (item, delta) in repeated.net.iter().enumerate() {
        next_marking[item] = add_amount(next_marking[item], delta);
    }
    let mut next_remaining = remaining.to_vec();
    for (recipe, count) in suffix.counts.iter().enumerate() {
        if let Some(count) = count.to_i64().filter(|count| *count > 0) {
            if let Some(left) = next_remaining[recipe].as_mut() {
                *left -= count * extra;
            }
        }
    }
    let body = trace_from(&flat[start..]);
    let folded = Rc::new(Trace::Repeat {
        previous: trace_before(&flat[..start]),
        body,
        width: (flat.len() - start) as u32,
        extra,
    });
    // `block` already contains the suffix once. Additional repetitions are all that gets appended.
    let added = if extra == 1 { suffix } else { suffix.repeated(extra - 1) };
    (Rc::new(block.then(&added)), folded, next_marking, next_remaining)
}

fn suffix_covered(suffix: &Block, marking: &[i64], importable: &[bool], times: i64) -> bool {
    let needed = suffix.repeated(times - 1);
    needed.required.iter().enumerate().all(|(item, amount)| {
        importable[item] || SignedAmount::from_i64(marking[item]) >= *amount
    })
}

fn trace_from(steps: &[Step]) -> Rc<Trace> {
    let mut trace = Rc::new(Trace::Root);
    for step in steps {
        trace = Rc::new(Trace::Step {
            previous: trace,
            recipe: step.recipe,
            count: step.count.to_i64().unwrap_or(0),
        });
    }
    trace
}

fn trace_before(steps: &[Step]) -> Rc<Trace> {
    trace_from(steps)
}

fn add_amount(marking: i64, delta: &SignedAmount) -> i64 {
    let updated = (marking as i128) + delta.to_i64().map(i128::from).unwrap_or(i128::from(i64::MAX));
    updated.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// Firings of one recipe that are worth taking from this marking.
///
/// Direct firings cover a target with this recipe's own gross output. Supporting firings cover the
/// gross input of another recipe that itself covers a target. Gross, not net: a recipe that consumes
/// a catalyst and returns it still has to hold the catalyst before it can fire.
fn outstanding_firings(recipes: &[Recipe], firing: usize, marking: &[i64], target: &[i64]) -> i64 {
    let direct = gross_demand(&recipes[firing], marking, target);
    let mut support = 0i64;
    for (other_index, other) in recipes.iter().enumerate() {
        if other_index == firing {
            continue;
        }
        let want = gross_demand(other, marking, target);
        if want <= 0 {
            continue;
        }
        for &(item, consumed) in &other.consumed {
            let growth = recipes[firing].net.iter().find(|(index, _)| *index == item).map(|(_, delta)| *delta).unwrap_or(0);
            if growth <= 0 {
                continue;
            }
            let needed = (consumed as i128).saturating_mul(want as i128);
            let shortage = needed.saturating_sub(marking[item] as i128);
            if shortage > 0 {
                let batches = (shortage + growth as i128 - 1) / growth as i128;
                support = support.max(batches.min(i64::MAX as i128) as i64);
            }
        }
    }
    direct.max(support).max(1)
}

fn gross_demand(recipe: &Recipe, marking: &[i64], target: &[i64]) -> i64 {
    let mut useful = 0i64;
    for &(item, produced) in &recipe.produced {
        if produced <= 0 {
            continue;
        }
        let outstanding = target[item].saturating_sub(marking[item]);
        if outstanding > 0 {
            useful = useful.max(outstanding.saturating_add(produced - 1) / produced);
        }
    }
    useful
}

/// Largest batch whose internal inputs never go negative. An importable input is not a limit:
/// ECO books it as external demand at replay time and lets the firing proceed.
fn safe_batch(recipe: &Recipe, marking: &[i64], remaining: i64, importable: &[bool]) -> i64 {
    maximum_safe(recipe, marking, remaining, importable)
}

fn maximum_safe(recipe: &Recipe, marking: &[i64], remaining: i64, importable: &[bool]) -> i64 {
    let mut safe = remaining;
    let mut constrained = false;
    for &(item, consumed) in &recipe.consumed {
        if consumed <= 0 || importable[item] {
            continue;
        }
        constrained = true;
        let available = marking[item];
        if available < consumed {
            return 0;
        }
        let delta = recipe.net.iter().find(|(index, _)| *index == item).map(|(_, delta)| *delta).unwrap_or(0);
        let batches = if delta < 0 {
            // m - c is what remains after the first firing; each later firing loses |delta|.
            (available - consumed) / -delta + 1
        } else if delta > 0 {
            // The first firing is enabled and every later firing leaves more than it consumes.
            remaining
        } else {
            available / consumed
        };
        safe = safe.min(batches);
    }
    // No internal input means the batch is bounded only by the caller's remaining count. An
    // uncapped one stays at a single firing, matching ECO's refusal to invent an unbounded edge.
    if !constrained {
        return if remaining == i64::MAX { 1 } else { remaining.max(0) };
    }
    safe.max(0)
}

fn candidates(recipes: &[Recipe], firing: usize, marking: &[i64], maximum: i64) -> Vec<i64> {
    let mut values = BTreeSet::new();
    values.insert(maximum);
    // A growing recipe stays enabled after its first firing, so firing it once and firing the whole
    // batch reach the same family of markings. Keeping both makes the witness one step per item.
    let grows = recipes[firing].net.iter().any(|&(_, delta)| delta > 0);
    if maximum > 1 && !grows {
        values.insert(1);
    }
    // A net-growing recipe passes every intermediate stock on its way to the maximum batch, and
    // the later recipe can fire there. Emitting those intermediates as separate edges unrolls one
    // batch into one step per item.
    if grows {
        return values.into_iter().collect();
    }
    for (waiting, recipe) in recipes.iter().enumerate() {
        if waiting == firing {
            continue;
        }
        if maximum_safe(recipe, marking, i64::MAX, &vec![false; marking.len()]) > 0 {
            continue;
        }
        let mut breakpoint = 0i64;
        let mut reachable = true;
        for &(item, needed) in &recipe.consumed {
            let shortage = needed.saturating_sub(marking[item]);
            if shortage <= 0 {
                continue;
            }
            let growth = recipes[firing].net.iter().find(|(index, _)| *index == item).map(|(_, delta)| *delta).unwrap_or(0);
            if growth <= 0 {
                reachable = false;
                break;
            }
            let batches = shortage.saturating_add(growth - 1) / growth;
            breakpoint = breakpoint.max(batches);
        }
        if reachable && (1..=maximum).contains(&breakpoint) {
            values.insert(breakpoint);
        }
    }
    values.into_iter().collect()
}

fn apply_batch(recipe: &Recipe, marking: &mut [i64], count: i64, importable: &[bool]) {
    for &(item, delta) in &recipe.net {
        // Boundary supply covers an importable input on demand, so the marking stays nonnegative
        // and the booked import is recovered from the block's prefix requirement afterwards.
        if importable[item] && delta < 0 {
            let consumed = recipe.consumed.iter().find(|(index, _)| *index == item).map(|(_, amount)| *amount).unwrap_or(0);
            let produced = consumed + delta;
            let first = (marking[item] - consumed).max(0) + produced;
            let updated = (first as i128) + (delta as i128) * ((count - 1) as i128);
            marking[item] = updated.clamp(0, i64::MAX as i128) as i64;
            continue;
        }
        let updated = (marking[item] as i128) + (delta as i128) * (count as i128);
        marking[item] = updated.clamp(0, i64::MAX as i128) as i64;
    }
}

fn satisfied(marking: &[i64], target: &[i64]) -> bool {
    marking.iter().zip(target).all(|(have, need)| have >= need)
}

fn deficit(marking: &[i64], target: &[i64]) -> u128 {
    marking.iter().zip(target).fold(0u128, |total, (have, need)| {
        total.saturating_add(need.saturating_sub(*have) as u128)
    })
}

/// Identity of a marking. Firing counts are not part of it: ECO closes the reachable marking set,
/// and two paths that arrive at the same stock are the same state.
fn signature_of(marking: &[i64]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for value in marking {
        hash ^= *value as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn sparse(column: &[i64]) -> Vec<(usize, i64)> {
    column.iter().enumerate().filter(|(_, value)| **value != 0).map(|(index, value)| (index, *value)).collect()
}

fn sparse_net(consumed: &[i64], produced: &[i64]) -> Vec<(usize, i64)> {
    consumed
        .iter()
        .zip(produced)
        .enumerate()
        .filter_map(|(item, (input, output))| {
            let delta = output.checked_sub(*input)?;
            (delta != 0).then_some((item, delta))
        })
        .collect()
}

fn net_columns(request: &SolveRequest) -> Vec<Vec<i64>> {
    request
        .consumed
        .iter()
        .zip(&request.produced)
        .map(|(consumed, produced)| consumed.iter().zip(produced).map(|(input, output)| output.saturating_sub(*input)).collect())
        .collect()
}

fn validate(request: &SolveRequest) -> Result<(), &'static str> {
    let items = request.stock.len();
    if request.target.len() != items || request.importable.len() != items {
        return Err("stock, target and import mask have different lengths");
    }
    if request.consumed.len() != request.produced.len() {
        return Err("consumption and production have different recipe counts");
    }
    for (consumed, produced) in request.consumed.iter().zip(&request.produced) {
        if consumed.len() != items || produced.len() != items {
            return Err("a recipe column has a different item count");
        }
        if consumed.iter().chain(produced.iter()).any(|amount| *amount < 0) {
            return Err("recipe amounts must be nonnegative");
        }
    }
    if request.stock.iter().chain(request.target.iter()).any(|amount| *amount < 0) {
        return Err("stock and target must be nonnegative");
    }
    if let Some(candidate) = &request.candidate {
        if candidate.len() != request.consumed.len() || candidate.iter().any(|count| *count < 0) {
            return Err("candidate firing vector is invalid");
        }
    }
    Ok(())
}

fn empty(status: Status, recipes: usize, items: usize) -> SolveResult {
    SolveResult {
        status,
        firings: vec![UnsignedAmount::zero(); recipes],
        seed: vec![SignedAmount::zero(); items],
        external: vec![SignedAmount::zero(); items],
        shortfall: vec![SignedAmount::zero(); items],
        steps: Vec::new(),
        intervals: Vec::new(),
        states: 0,
        startup: Vec::new(),
    }
}

/// The internal input each recipe still lacks, with dominated alternatives removed.
fn startup_candidates(request: &SolveRequest) -> Vec<Vec<SignedAmount>> {
    let items = request.stock.len();
    let mut frontier: Vec<Vec<i64>> = Vec::new();
    for recipe in 0..request.consumed.len() {
        let missing: Vec<i64> = (0..items)
            .map(|item| {
                if request.importable[item] {
                    0
                } else {
                    (request.consumed[recipe][item] - request.stock[item]).max(0)
                }
            })
            .collect();
        if missing.iter().all(|amount| *amount == 0) {
            continue;
        }
        if frontier.iter().any(|existing| dominates(existing, &missing)) {
            continue;
        }
        frontier.retain(|existing| !dominates(&missing, existing));
        frontier.push(missing);
    }
    frontier.into_iter().map(|missing| missing.into_iter().map(SignedAmount::from_i64).collect()).collect()
}

fn dominates(left: &[i64], right: &[i64]) -> bool {
    left.iter().zip(right).all(|(left, right)| left <= right)
}

fn with_intervals(status: Status, recipes: usize, items: usize, intervals: Vec<FiringInterval>) -> SolveResult {
    let mut result = empty(status, recipes, items);
    result.intervals = intervals;
    result
}
