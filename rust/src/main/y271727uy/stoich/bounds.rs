//! Firing-count intervals from the material balance, before any search.
//!
//! Rows are `stock + net·x >= target`, rearranged to `(-net)·x <= stock - target`.
//! Coefficients are reduced by their gcd and the right-hand side is rounded so an integer
//! left-hand side still implies it. Propagation follows Savelsbergh's bound probing, with the
//! same finite pass limit ECO uses: a net-consuming feedback loop can raise a lower bound
//! forever, and a finite pass still substitutes only quantities forced in every feasible vector.

use crate::quantity::UnsignedAmount;
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{Signed, ToPrimitive, Zero};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FiringInterval {
    pub lower: UnsignedAmount,
    pub upper: Option<UnsignedAmount>,
}

impl FiringInterval {
    pub fn unbounded() -> Self {
        Self { lower: UnsignedAmount::zero(), upper: None }
    }

    pub fn fixed(value: UnsignedAmount) -> Self {
        Self { lower: value.clone(), upper: Some(value) }
    }

    pub fn is_fixed(&self) -> bool {
        self.upper.as_ref().is_some_and(|upper| upper == &self.lower)
    }

    /// Whether a firing count is inside the interval. An open upper end accepts any count at or
    /// above the lower end.
    pub fn contains(&self, count: i64) -> bool {
        if count < 0 {
            return false;
        }
        let count = UnsignedAmount::Small(count);
        count >= self.lower && self.upper.as_ref().is_none_or(|upper| count <= *upper)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Bounds {
    /// Every feasible vector lies inside these intervals.
    Intervals(Vec<FiringInterval>),
    /// The balance itself has no nonnegative solution. This is a proof, not a budget cut.
    Infeasible,
}

#[derive(Clone, Copy)]
struct Term {
    recipe: usize,
    coefficient: i64,
}

struct Row {
    terms: Vec<Term>,
    bound: BigInt,
}

/// `net[recipe][item]` is production minus consumption. `importable` items are omitted: their
/// supply is charged later as an external demand, exactly as ECO skips boundary keys.
pub fn propagate(
    net: &[Vec<i64>],
    stock: &[i64],
    target: &[i64],
    importable: &[bool],
    candidate: Option<&[i64]>,
) -> Result<Bounds, &'static str> {
    let recipes = net.len();
    if stock.len() != target.len() || importable.len() != stock.len() {
        return Err("stock, target and import mask have different lengths");
    }
    if net.iter().any(|column| column.len() != stock.len()) {
        return Err("a recipe net column has a different item count");
    }
    for value in stock.iter().chain(target.iter()) {
        if *value < 0 {
            return Err("stock and target must be nonnegative");
        }
    }

    let mut rows = Vec::new();
    for item in 0..stock.len() {
        if importable[item] {
            continue;
        }
        let mut terms = Vec::new();
        for recipe in 0..recipes {
            let coefficient = net[recipe][item];
            if coefficient != 0 {
                // stock - target >= -net·x, so the probed inequality is (-net)·x <= stock - target.
                terms.push(Term { recipe, coefficient: coefficient.saturating_neg() });
            }
        }
        let bound = BigInt::from(stock[item]) - BigInt::from(target[item]);
        if terms.is_empty() {
            if bound < BigInt::zero() {
                return Ok(Bounds::Infeasible);
            }
            continue;
        }
        rows.push(normalize(terms, bound));
    }

    if let Some(counts) = candidate {
        if counts.len() != recipes {
            return Err("candidate firing vector has a different recipe count");
        }
        for (recipe, count) in counts.iter().enumerate() {
            if *count < 0 {
                return Err("candidate firing count is negative");
            }
            if *count > 0 {
                rows.push(Row {
                    terms: vec![Term { recipe, coefficient: -1 }],
                    bound: BigInt::from(-*count),
                });
            }
        }
    }

    match tighten(recipes, rows)? {
        None => Ok(Bounds::Infeasible),
        Some(intervals) => Ok(Bounds::Intervals(intervals)),
    }
}

fn normalize(terms: Vec<Term>, bound: BigInt) -> Row {
    let mut gcd = BigInt::zero();
    for term in &terms {
        gcd = gcd.gcd(&BigInt::from(term.coefficient).abs());
    }
    if gcd.is_zero() || gcd == BigInt::from(1) {
        return Row { terms, bound };
    }
    let terms = terms
        .into_iter()
        .map(|term| Term {
            recipe: term.recipe,
            coefficient: reduced_coefficient(BigInt::from(term.coefficient) / &gcd),
        })
        .collect();
    let (quotient, remainder) = bound.div_mod_floor(&gcd);
    // An integer left-hand side is a multiple of the gcd, including when the remainder is negative.
    let bound = if remainder.sign() == num_bigint::Sign::Minus { quotient - 1 } else { quotient };
    Row { terms, bound }
}

fn reduced_coefficient(value: BigInt) -> i64 {
    value.to_i64().expect("reduced recipe coefficient fits i64")
}

fn tighten(recipes: usize, rows: Vec<Row>) -> Result<Option<Vec<FiringInterval>>, &'static str> {
    let mut lower = vec![BigInt::zero(); recipes];
    let mut upper: Vec<Option<BigInt>> = vec![None; recipes];
    let mut passes = 0usize;
    loop {
        let mut changed = false;
        for row in &rows {
            let mut minimum = BigInt::zero();
            let mut unbounded = 0usize;
            let mut contributions: Vec<Option<BigInt>> = Vec::with_capacity(row.terms.len());
            for term in &row.terms {
                let extreme = if term.coefficient > 0 {
                    Some(lower[term.recipe].clone())
                } else {
                    upper[term.recipe].clone()
                };
                match extreme {
                    Some(bound) => {
                        let contribution = &bound * term.coefficient;
                        minimum += &contribution;
                        contributions.push(Some(contribution));
                    }
                    None => {
                        unbounded += 1;
                        contributions.push(None);
                    }
                }
            }
            if unbounded == 0 && minimum > row.bound {
                return Ok(None);
            }
            for (index, term) in row.terms.iter().enumerate() {
                let this_unbounded = usize::from(contributions[index].is_none());
                if unbounded - this_unbounded != 0 {
                    continue;
                }
                let others = match &contributions[index] {
                    Some(contribution) => &minimum - contribution,
                    None => minimum.clone(),
                };
                let rhs = &row.bound - others;
                if term.coefficient > 0 {
                    let bound = floor_div(&rhs, &BigInt::from(term.coefficient));
                    if upper[term.recipe].as_ref().is_none_or(|current| bound < *current) {
                        upper[term.recipe] = Some(bound);
                        changed = true;
                    }
                } else {
                    let bound = -floor_div(&rhs, &BigInt::from(-term.coefficient));
                    if bound > lower[term.recipe] {
                        lower[term.recipe] = bound;
                        changed = true;
                    }
                }
                if let Some(high) = &upper[term.recipe] {
                    if lower[term.recipe] > *high {
                        return Ok(None);
                    }
                }
            }
        }
        passes += 1;
        if !changed || passes > recipes {
            break;
        }
    }

    if lower.iter().any(|value| value.sign() == num_bigint::Sign::Minus) {
        return Err("propagation produced a negative firing lower bound");
    }
    Ok(Some(
        lower
            .into_iter()
            .zip(upper)
            .map(|(low, high)| FiringInterval { lower: unsigned(low), upper: high.map(unsigned) })
            .collect(),
    ))
}

fn floor_div(numerator: &BigInt, positive_denominator: &BigInt) -> BigInt {
    numerator.div_floor(positive_denominator)
}

fn unsigned(value: BigInt) -> UnsignedAmount {
    match value.to_i64() {
        Some(small) if small >= 0 => UnsignedAmount::Small(small),
        _ => UnsignedAmount::Big(value),
    }
}
