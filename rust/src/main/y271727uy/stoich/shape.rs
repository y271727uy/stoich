//! Closed forms for the two shapes every planner already special-cases.
//!
//! A single self-growing recipe has no interleaving, so its firing count and seed are arithmetic.
//! A fixed multi-step order is one block repeated by the same affine prefix rule. Anything else
//! returns [`Shape::NotApplicable`] and the caller continues with the general search.

use crate::quantity::{SignedAmount, UnsignedAmount};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Shape {
    pub firings: Vec<UnsignedAmount>,
    /// Maximum prefix deficit of every item, including externally supplied ones.
    pub seed: Vec<SignedAmount>,
    pub net: Vec<SignedAmount>,
}

#[derive(Debug)]
pub enum ShapeMiss {
    NotApplicable,
    Unrepresentable,
}

/// One recipe that consumes and returns the same item, with positive net growth.
///
/// The seed of the feedback item is its consumption, independent of the firing count. External
/// inputs scale with the count. Items the pattern does not net-produce cannot be targets.
pub fn single_growth(
    consumed: &[i64],
    produced: &[i64],
    stock: &[i64],
    target: &[i64],
) -> Result<Shape, ShapeMiss> {
    if consumed.len() != produced.len() || consumed.len() != stock.len() || stock.len() != target.len() {
        return Err(ShapeMiss::NotApplicable);
    }
    let mut feedback = None;
    for item in 0..consumed.len() {
        if consumed[item] < 0 || produced[item] < 0 || stock[item] < 0 || target[item] < 0 {
            return Err(ShapeMiss::NotApplicable);
        }
        if consumed[item] > 0 && produced[item] > 0 {
            if feedback.is_some() {
                return Err(ShapeMiss::NotApplicable);
            }
            feedback = Some(item);
        }
    }
    let Some(feedback) = feedback else {
        return Err(ShapeMiss::NotApplicable);
    };
    let consume = consumed[feedback];
    let produce = produced[feedback];
    let growth = produce.checked_sub(consume).ok_or(ShapeMiss::Unrepresentable)?;
    if growth <= 0 {
        return Err(ShapeMiss::NotApplicable);
    }

    let mut firings = UnsignedAmount::zero();
    let mut outstanding = false;
    for item in 0..consumed.len() {
        let need = target[item].saturating_sub(stock[item]);
        if need <= 0 {
            continue;
        }
        outstanding = true;
        let per_firing = produced[item].checked_sub(consumed[item]).ok_or(ShapeMiss::Unrepresentable)?;
        if per_firing <= 0 {
            return Err(ShapeMiss::NotApplicable);
        }
        let count = UnsignedAmount::Small(need)
            .ceil_div(&UnsignedAmount::Small(per_firing))
            .ok_or(ShapeMiss::Unrepresentable)?;
        if count > firings {
            firings = count;
        }
    }
    if !outstanding {
        return Err(ShapeMiss::NotApplicable);
    }

    let mut seed = vec![SignedAmount::zero(); consumed.len()];
    seed[feedback] = SignedAmount::from_i64(consume);
    for item in 0..consumed.len() {
        if item == feedback {
            continue;
        }
        let net_input = consumed[item].saturating_sub(produced[item]);
        if net_input > 0 {
            seed[item] = SignedAmount::from_i64(net_input).checked_mul(&firings);
        }
    }
    let mut net = Vec::with_capacity(consumed.len());
    for item in 0..consumed.len() {
        let delta = produced[item].checked_sub(consumed[item]).ok_or(ShapeMiss::Unrepresentable)?;
        net.push(SignedAmount::from_i64(delta).checked_mul(&firings));
    }
    Ok(Shape { firings: vec![firings], seed, net })
}

/// A caller-supplied order, repeated until the target net is covered.
///
/// `order` is one cycle: each entry is `(recipe, count)`. The cycle net must produce the target,
/// and every named count must be positive. The returned seed is the maximum prefix deficit of the
/// repeated order, so a negative cycle slope is paid once per extra repetition.
pub fn fixed_order(
    consumed: &[Vec<i64>],
    produced: &[Vec<i64>],
    order: &[(usize, i64)],
    stock: &[i64],
    target_item: usize,
    target_amount: i64,
) -> Result<Shape, ShapeMiss> {
    if order.is_empty() || target_amount <= 0 {
        return Err(ShapeMiss::NotApplicable);
    }
    let items = stock.len();
    let recipes = consumed.len();
    if produced.len() != recipes || target_item >= items {
        return Err(ShapeMiss::NotApplicable);
    }
    let unit = block_of(consumed, produced, order).map_err(|_| ShapeMiss::NotApplicable)?;
    let effect = unit.net[target_item].to_i64().ok_or(ShapeMiss::Unrepresentable)?;
    if effect <= 0 {
        return Err(ShapeMiss::NotApplicable);
    }
    let outstanding = target_amount.saturating_sub(stock[target_item]).max(0);
    let repetitions = UnsignedAmount::Small(outstanding)
        .ceil_div(&UnsignedAmount::Small(effect))
        .ok_or(ShapeMiss::Unrepresentable)?;
    let repetitions = if repetitions.is_zero() { UnsignedAmount::one() } else { repetitions };

    let repeated = unit.repeat(&repetitions);
    let mut firings = vec![UnsignedAmount::zero(); recipes];
    for (recipe, count) in order {
        let add = UnsignedAmount::try_from_i64(*count).ok_or(ShapeMiss::NotApplicable)?.checked_mul(&repetitions);
        firings[*recipe] = firings[*recipe].checked_add(&add);
    }
    Ok(Shape { firings, seed: repeated.required, net: repeated.net })
}

#[derive(Clone)]
struct OrderBlock {
    required: Vec<SignedAmount>,
    net: Vec<SignedAmount>,
}

impl OrderBlock {
    fn repeat(&self, times: &UnsignedAmount) -> Self {
        let extra = times.saturating_sub(&UnsignedAmount::one());
        let mut required = self.required.clone();
        let mut net = Vec::with_capacity(self.net.len());
        for item in 0..self.net.len() {
            if self.net[item].signum() < 0 {
                let slope = SignedAmount::zero().checked_sub(&self.net[item]);
                required[item] = required[item].checked_add(&slope.checked_mul(&extra));
            }
            net.push(self.net[item].checked_mul(times));
        }
        Self { required, net }
    }
}

fn block_of(consumed: &[Vec<i64>], produced: &[Vec<i64>], order: &[(usize, i64)]) -> Result<OrderBlock, ()> {
    let items = consumed.first().map(Vec::len).unwrap_or(0);
    let mut required = vec![SignedAmount::zero(); items];
    let mut balance = vec![SignedAmount::zero(); items];
    for &(recipe, count) in order {
        if count <= 0 || recipe >= consumed.len() || consumed[recipe].len() != items || produced[recipe].len() != items {
            return Err(());
        }
        let count_amount = UnsignedAmount::Small(count);
        for item in 0..items {
            let input = consumed[recipe][item];
            if input < 0 || produced[recipe][item] < 0 {
                return Err(());
            }
            let delta = produced[recipe][item].checked_sub(input).ok_or(())?;
            let mut needed = SignedAmount::from_i64(input);
            if delta < 0 {
                let loss = SignedAmount::from_i64(-delta).checked_mul(&UnsignedAmount::Small(count - 1));
                needed = needed.checked_add(&loss);
            }
            let deficit = needed.checked_sub(&balance[item]);
            if deficit.signum() > 0 {
                balance[item] = balance[item].checked_add(&deficit);
                required[item] = required[item].checked_add(&deficit);
            }
            balance[item] = balance[item].checked_add(&SignedAmount::from_i64(delta).checked_mul(&count_amount));
        }
    }
    let net = balance
        .iter()
        .zip(&required)
        .map(|(final_balance, seed)| final_balance.checked_sub(seed))
        .collect();
    Ok(OrderBlock { required, net })
}
