use crate::balance::{self, Balance};
use crate::{fixed_order, propagate, single_growth, solve, Bounds, SolveRequest, Status};

fn growth_request(stock: i64, target: i64) -> SolveRequest {
    SolveRequest {
        consumed: vec![vec![9, 1]],
        produced: vec![vec![10, 0]],
        stock: vec![stock, 0],
        target: vec![target, 0],
        importable: vec![false, true],
        candidate: None,
        order: None,
        target_item: None,
        max_states: 1_000,
        skip_equation: false,
    }
}

#[test]
fn single_recipe_growth_is_closed_form() {
    let result = solve(&growth_request(9, 1_000_000_009)).unwrap();
    assert_eq!(result.status, Status::Success);
    assert_eq!(result.firings[0].to_i64(), Some(1_000_000_000));
    assert_eq!(result.seed[0].to_i64(), Some(9));
    assert_eq!(result.external[1].to_i64(), Some(1_000_000_000));
    assert_eq!(result.states, 0, "growth never enters the marking search");
}

#[test]
fn growth_seed_does_not_scale_with_firings() {
    let small = single_growth(&[9], &[10], &[0], &[10]).unwrap();
    let large = single_growth(&[9], &[10], &[0], &[1_000_000_000]).unwrap();
    assert_eq!(small.seed[0], large.seed[0]);
    assert_eq!(large.firings[0].to_i64(), Some(1_000_000_000));
}

#[test]
fn missing_growth_seed_is_reported_exactly() {
    let result = solve(&growth_request(4, 14)).unwrap();
    assert_eq!(result.status, Status::InsufficientSeed);
    assert_eq!(result.firings[0].to_i64(), Some(10));
    assert_eq!(result.shortfall[0].to_i64(), Some(5));
}

#[test]
fn the_state_equation_orders_a_vector_the_stock_can_fire() {
    // Balance forces four of the cheap recipe. The replay has to produce the catalyst before the
    // product recipe can spend it, and the seed is the one catalyst the stock already holds.
    let request = SolveRequest {
        consumed: vec![vec![1, 0, 0], vec![1, 0, 0]],
        produced: vec![vec![2, 0, 0], vec![0, 1, 0]],
        stock: vec![1, 0, 5],
        target: vec![1, 4, 0],
        importable: vec![false, false, true],
        candidate: None,
        order: None,
        target_item: None,
        max_states: 1_000,
        skip_equation: false,
    };
    let result = solve(&request).unwrap();
    assert_eq!(result.status, Status::Success, "{result:?}");
    assert_eq!(result.firings[1].to_i64(), Some(4));
    assert_eq!(result.states, 0, "the equation witness does not search markings");
}

#[test]
fn a_batch_reserves_the_catalysts_it_holds_at_once() {
    // The recipe returns its catalyst, so sequential replay would hold one. Firing twice in a batch
    // holds two at the same moment, and the stock has them, so the seed is two.
    let request = SolveRequest {
        consumed: vec![vec![1, 0]],
        produced: vec![vec![1, 1]],
        stock: vec![2, 0],
        target: vec![2, 2],
        importable: vec![false, false],
        candidate: None,
        order: None,
        target_item: None,
        max_states: 1_000,
        skip_equation: false,
    };
    let result = solve(&request).unwrap();
    assert_eq!(result.status, Status::Success, "{result:?}");
    assert_eq!(result.seed[0].to_i64(), Some(2));
}

#[test]
fn a_stuck_witness_raises_the_least_missing_seed() {
    // Nothing is enabled from an empty stock. The witness proposes the one catalyst the growth
    // recipe needs, then replays, and the result reports that catalyst as a seed rather than owned.
    let request = SolveRequest {
        consumed: vec![vec![1, 0], vec![1, 0]],
        produced: vec![vec![2, 0], vec![0, 1]],
        stock: vec![0, 0],
        target: vec![0, 2],
        importable: vec![false, false],
        candidate: None,
        order: None,
        target_item: None,
        max_states: 1_000,
        skip_equation: false,
    };
    let result = solve(&request).unwrap();
    assert_eq!(result.status, Status::InsufficientSeed, "{result:?}");
    assert_eq!(result.seed[0].to_i64(), Some(1));
    assert!(!result.startup.is_empty(), "a proven shortage names what would start it");
}

#[test]
fn two_recipe_ring_folds_instead_of_unrolling() {
    // A recipe grows the catalyst; B turns one catalyst into the requested product.
    let request = SolveRequest {
        consumed: vec![vec![1, 0, 0], vec![1, 0, 0]],
        produced: vec![vec![2, 0, 0], vec![0, 1, 0]],
        stock: vec![1, 0, 0],
        target: vec![0, 8, 0],
        importable: vec![false, false, false],
        candidate: None,
        order: None,
        target_item: None,
        max_states: 1_000,
        skip_equation: false,
    };
    let result = solve(&request).unwrap();
    assert_eq!(result.status, Status::Success);
    assert_eq!(result.firings[0].to_i64(), Some(7));
    assert_eq!(result.firings[1].to_i64(), Some(8));
    assert!(result.steps.len() <= 2, "one growth batch and one product batch, got {} steps: {:?}", result.steps.len(), result.steps);
    assert!(result.states <= 4, "the run is closed form, got {} states", result.states);
}

#[test]
fn fixed_order_seed_uses_the_negative_slope_once_per_repetition() {
    // One cycle consumes 2 fuel and grows the catalyst by 1. Four cycles need the unit seed
    // plus three extra fuel deficits.
    let shape = fixed_order(&[vec![1, 2]], &[vec![2, 0]], &[(0, 1)], &[0, 0], 0, 4).unwrap();
    assert_eq!(shape.firings[0].to_i64(), Some(4));
    assert_eq!(shape.seed[0].to_i64(), Some(1));
    assert_eq!(shape.seed[1].to_i64(), Some(8));
}

#[test]
fn balance_proves_infeasible_without_search() {
    let request = SolveRequest {
        consumed: vec![vec![1, 0]],
        produced: vec![vec![0, 1]],
        stock: vec![0, 0],
        target: vec![0, 1],
        importable: vec![false, false],
        candidate: None,
        order: None,
        target_item: None,
        max_states: 10,
        skip_equation: false,
    };
    let result = solve(&request).unwrap();
    assert_eq!(result.status, Status::Infeasible);
}

#[test]
fn propagation_rounds_through_the_row_gcd() {
    let net = vec![vec![2i64, 0], vec![0, 2]];
    let bounds = propagate(&net, &[0, 0], &[1, 0], &[false, false], None).unwrap();
    let Bounds::Intervals(intervals) = bounds else { panic!("expected intervals") };
    assert_eq!(intervals[0].lower.to_i64(), Some(1));
}

#[test]
fn propagation_stops_on_a_finite_pass() {
    // Each firing consumes one of the target and produces nothing else. Lower bounds rise, but
    // only for the finite number of passes; the row stays feasible at the zero vector's complement.
    let net = vec![vec![-1i64]];
    let bounds = propagate(&net, &[0], &[1], &[false], None).unwrap();
    assert!(matches!(bounds, Bounds::Infeasible));
}

fn dead_end() -> SolveRequest {
    // Each recipe replaces what the other consumes, so material balance has solutions. With empty
    // stock no firing is enabled, and only the closed marking set proves the target is unreachable.
    SolveRequest {
        consumed: vec![vec![1, 0], vec![0, 1]],
        produced: vec![vec![0, 1], vec![1, 0]],
        stock: vec![0, 0],
        target: vec![0, 2],
        importable: vec![false, false],
        candidate: None,
        order: None,
        target_item: None,
        max_states: 1_000,
        skip_equation: false,
    }
}

#[test]
fn closed_reachable_set_proves_the_stock_is_short() {
    // ECO reports a proven shortage only after every reachable marking has been expanded.
    let result = solve(&dead_end()).unwrap();
    assert_eq!(result.status, Status::InsufficientSeed);
    assert!(result.states < 1_000, "the closure is finite, got {} states", result.states);
}

#[test]
fn a_budget_cut_proves_nothing() {
    // Two recipes feed each other and both grow, so there is no single-recipe closed form and the
    // reachable set does not close. Two states are not an exhaustive search, so nothing is proven.
    let request = SolveRequest {
        consumed: vec![vec![1, 0], vec![0, 1]],
        produced: vec![vec![1, 2], vec![2, 1]],
        stock: vec![1, 1],
        target: vec![40, 40],
        importable: vec![false, false],
        candidate: None,
        order: None,
        target_item: None,
        max_states: 2,
        // The material balance of two mutually growing recipes has a finite minimum, so the equation
        // witness would answer before any search. Skip it to exercise the truncated marking search.
        skip_equation: true,
    };
    let result = solve(&request).unwrap();
    assert_eq!(result.status, Status::BudgetExhausted, "firings={:?} states={}", result.firings, result.states);
}

#[test]
fn importable_input_is_booked_instead_of_blocking() {
    // One iron is imported on demand. The catalyst still has to be owned, so the seed stays one.
    let request = SolveRequest {
        consumed: vec![vec![1, 1]],
        produced: vec![vec![2, 0]],
        stock: vec![1, 0],
        target: vec![8, 0],
        importable: vec![false, true],
        candidate: None,
        order: None,
        target_item: None,
        max_states: 1_000,
        skip_equation: false,
    };
    let result = solve(&request).unwrap();
    assert_eq!(result.status, Status::Success);
    assert_eq!(result.firings[0].to_i64(), Some(7));
    assert_eq!(result.seed[0].to_i64(), Some(1));
    assert_eq!(result.external[1].to_i64(), Some(7));
}

#[test]
fn simplex_counts_a_catalyst_ring() {
    // Growing the catalyst by one per firing and spending one per product needs four of each to
    // finish with the catalyst intact and four product.
    let net = vec![vec![1, 0, -1], vec![-1, 1, 0]];
    let balance = balance::minimize(&net, &[1, 0, 5], &[1, 4, 0], &[false, false, true]);
    let Balance::Optimal(counts) = balance else { panic!("expected an optimum, got {balance:?}"); };
    assert_eq!(counts[0], 4.into());
    assert_eq!(counts[1], 4.into());
}

#[test]
fn simplex_minimizes_a_split_output() {
    // Either recipe yields the product. The first consumes one input per firing, the second two,
    // so the minimum is two firings of the first.
    let net = vec![vec![1, -1], vec![1, -2]];
    let balance = balance::minimize(&net, &[0, 4], &[2, 0], &[false, false]);
    let Balance::Optimal(counts) = balance else { panic!("expected an optimum, got {balance:?}"); };
    assert_eq!(counts[0], 2.into());
    assert_eq!(counts[1], 0.into());
}

#[test]
fn simplex_proves_every_branch_infeasible() {
    let net = vec![vec![-1, 1]];
    assert_eq!(balance::minimize(&net, &[0, 0], &[0, 1], &[false, false]), Balance::Infeasible);
}

#[test]
fn rejects_a_negative_recipe_amount() {
    let mut request = growth_request(9, 10);
    request.consumed[0][0] = -1;
    assert!(solve(&request).is_err());
}
