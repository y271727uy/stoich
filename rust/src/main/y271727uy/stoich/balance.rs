//! Exact material balance, transcribed from ECO's `CycleStateEquation`.
//!
//! The state equation `stock + (produced - consumed) x >= target`, `x >= 0`, is necessary and not a
//! firing order. A two-phase simplex minimizes the total firing count and branches on the first
//! fractional variable. Infeasibility is a proof only after every branch closes. An importable item
//! is skipped: its deficit is booked later, not constrained here.

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, Zero};

const BRANCH_LIMIT: usize = 4_096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Balance {
    /// Every branch closed without an integer vector.
    Infeasible,
    /// The minimum total firing vector.
    Optimal(Vec<BigInt>),
    /// A feasible integer vector found before the branch budget closed.
    Feasible(Vec<BigInt>),
    /// The branch budget closed with nothing proven.
    Unknown,
}

#[derive(Clone)]
struct Constraint {
    indices: Vec<usize>,
    coefficients: Vec<BigInt>,
    bound: BigInt,
}

#[derive(Clone, PartialEq, Eq)]
struct Rational {
    numerator: BigInt,
    denominator: BigInt,
}

impl Rational {
    fn new(numerator: BigInt, denominator: BigInt) -> Self {
        let (numerator, denominator) = if denominator.is_negative() {
            (-numerator, -denominator)
        } else {
            (numerator, denominator)
        };
        let gcd = numerator.gcd(&denominator);
        Self { numerator: numerator / &gcd, denominator: denominator / gcd }
    }

    fn from_int(value: BigInt) -> Self {
        Self::new(value, BigInt::one())
    }

    fn zero() -> Self {
        Self::from_int(BigInt::zero())
    }

    fn one() -> Self {
        Self::from_int(BigInt::one())
    }

    fn negative_one() -> Self {
        Self::from_int(-BigInt::one())
    }

    fn signum(&self) -> i8 {
        if self.numerator.is_positive() {
            1
        } else if self.numerator.is_negative() {
            -1
        } else {
            0
        }
    }

    fn is_integer(&self) -> bool {
        self.denominator.is_one()
    }

    fn negate(&self) -> Self {
        Self::new(-&self.numerator, self.denominator.clone())
    }

    fn add(&self, other: &Self) -> Self {
        if other.signum() == 0 {
            return self.clone();
        }
        if self.signum() == 0 {
            return other.clone();
        }
        let gcd = self.denominator.gcd(&other.denominator);
        let left = &other.denominator / &gcd;
        let right = &self.denominator / &gcd;
        Self::new(&self.numerator * &left + &other.numerator * &right, &self.denominator * &left)
    }

    fn sub(&self, other: &Self) -> Self {
        self.add(&other.negate())
    }

    fn mul(&self, other: &Self) -> Self {
        if self.signum() == 0 || other.signum() == 0 {
            return Self::zero();
        }
        let left = self.numerator.gcd(&other.denominator);
        let right = other.numerator.gcd(&self.denominator);
        Self::new(
            (&self.numerator / &left) * (&other.numerator / &right),
            (&self.denominator / &right) * (&other.denominator / &left),
        )
    }

    fn div(&self, other: &Self) -> Self {
        self.mul(&Self::new(other.denominator.clone(), other.numerator.clone()))
    }
}

impl Rational {
    fn cmp_value(&self, other: &Self) -> std::cmp::Ordering {
        (&self.numerator * &other.denominator).cmp(&(&other.numerator * &self.denominator))
    }
}

/// Minimize the total firing count. `net[recipe][item]` is produced minus consumed.
pub fn minimize(net: &[Vec<i64>], stock: &[i64], target: &[i64], importable: &[bool]) -> Balance {
    let variables = net.len();
    let items = stock.len();
    let mut rows = vec![vec![BigInt::zero(); variables]; items];
    for (recipe, column) in net.iter().enumerate() {
        for (item, delta) in column.iter().enumerate() {
            if *delta != 0 {
                // ECO stores consumed minus produced, so a positive coefficient consumes stock.
                rows[item][recipe] = BigInt::from(-delta);
            }
        }
    }
    let mut constraints = Vec::new();
    for item in 0..items {
        if importable[item] {
            continue;
        }
        let row = &rows[item];
        if row.iter().all(is_zero) {
            if stock[item] < target[item] {
                return Balance::Infeasible;
            }
            continue;
        }
        // No recipe consumes it and stock already covers it, so the row cannot fail.
        if row.iter().all(|coefficient| !coefficient.is_positive()) && stock[item] >= target[item] {
            continue;
        }
        constraints.push(normalized(row, BigInt::from(stock[item] - target[item])));
    }
    presolve(variables, constraints)
}

fn is_zero(value: &BigInt) -> bool {
    Zero::is_zero(value)
}

fn normalized(row: &[BigInt], bound: BigInt) -> Constraint {
    let mut gcd = BigInt::zero();
    for coefficient in row {
        if !is_zero(coefficient) {
            gcd = gcd.gcd(coefficient);
        }
    }
    if is_zero(&gcd) {
        gcd = BigInt::one();
    }
    let (indices, coefficients): (Vec<_>, Vec<_>) = row
        .iter()
        .enumerate()
        .filter(|(_, coefficient)| !is_zero(coefficient))
        .map(|(index, coefficient)| (index, coefficient / &gcd))
        .unzip();
    // `div_mod_floor` already rounds a negative remainder down, so the quotient is the bound.
    let (quotient, _) = bound.div_mod_floor(&gcd);
    Constraint { indices, coefficients, bound: quotient }
}

fn presolve(variables: usize, constraints: Vec<Constraint>) -> Balance {
    let mut lower = vec![BigInt::zero(); variables];
    let mut upper = vec![None; variables];
    let mut passes = 0usize;
    loop {
        let mut changed = false;
        for row in &constraints {
            let mut minimum = BigInt::zero();
            let mut terms = vec![None; row.indices.len()];
            let mut unbounded = 0usize;
            for (index, (&variable, coefficient)) in row.indices.iter().zip(&row.coefficients).enumerate() {
                let bound = if coefficient.is_positive() { Some(lower[variable].clone()) } else { upper[variable].clone() };
                match bound {
                    Some(bound) => {
                        let term = coefficient * bound;
                        minimum += &term;
                        terms[index] = Some(term);
                    }
                    None => unbounded += 1,
                }
            }
            if unbounded == 0 && minimum > row.bound {
                return Balance::Infeasible;
            }
            for (index, (&variable, coefficient)) in row.indices.iter().zip(&row.coefficients).enumerate() {
                let missing = if terms[index].is_none() { 1 } else { 0 };
                if unbounded - missing != 0 {
                    continue;
                }
                let other = match &terms[index] {
                    Some(term) => &minimum - term,
                    None => minimum.clone(),
                };
                let rhs = &row.bound - other;
                let bound = if coefficient.is_positive() {
                    floor_div(&rhs, coefficient)
                } else {
                    -floor_div(&rhs, &-coefficient)
                };
                if coefficient.is_positive() && upper[variable].as_ref().is_none_or(|current| bound < *current) {
                    upper[variable] = Some(bound);
                    changed = true;
                } else if coefficient.is_negative() && bound > lower[variable] {
                    lower[variable] = bound;
                    changed = true;
                }
                if upper[variable].as_ref().is_some_and(|current| lower[variable] > *current) {
                    return Balance::Infeasible;
                }
            }
        }
        passes += 1;
        if !changed || passes > variables {
            break;
        }
    }
    let mut remap = vec![0usize; variables];
    let mut retained = 0usize;
    for variable in 0..variables {
        if lower[variable] == upper[variable].clone().unwrap_or(BigInt::from(-1)) && upper[variable].is_some() {
            remap[variable] = usize::MAX;
        } else {
            remap[variable] = retained;
            retained += 1;
        }
    }
    if retained == variables {
        return minimize_branches(variables, constraints);
    }
    let mut reduced = Vec::new();
    for row in &constraints {
        let mut coefficients = vec![BigInt::zero(); retained];
        let mut bound = row.bound.clone();
        let mut present = false;
        for (&variable, coefficient) in row.indices.iter().zip(&row.coefficients) {
            if remap[variable] == usize::MAX {
                bound -= coefficient * &lower[variable];
            } else {
                coefficients[remap[variable]] = coefficient.clone();
                present = true;
            }
        }
        if present {
            reduced.push(normalized(&coefficients, bound));
        } else if bound.is_negative() {
            return Balance::Infeasible;
        }
    }
    let reduced = minimize_branches(retained, reduced);
    let counts = match &reduced {
        Balance::Optimal(counts) | Balance::Feasible(counts) => counts,
        Balance::Infeasible => return Balance::Infeasible,
        Balance::Unknown => return Balance::Unknown,
    };
    let full: Vec<BigInt> = (0..variables)
        .map(|variable| if remap[variable] == usize::MAX { lower[variable].clone() } else { counts[remap[variable]].clone() })
        .collect();
    match reduced {
        Balance::Feasible(_) => Balance::Feasible(full),
        _ => Balance::Optimal(full),
    }
}

fn floor_div(numerator: &BigInt, positive_denominator: &BigInt) -> BigInt {
    let (quotient, remainder) = numerator.div_mod_floor(positive_denominator);
    if remainder.is_negative() { quotient - 1 } else { quotient }
}

fn minimize_branches(variables: usize, constraints: Vec<Constraint>) -> Balance {
    if variables == 0 {
        return Balance::Optimal(Vec::new());
    }
    let mut pending = vec![constraints];
    let mut best: Option<(BigInt, Vec<BigInt>)> = None;
    let mut explored = 0usize;
    while let Some(branch) = pending.pop() {
        explored += 1;
        if explored > BRANCH_LIMIT {
            return match best {
                Some((_, counts)) => Balance::Feasible(counts),
                None => Balance::Unknown,
            };
        }
        let Some(vector) = Tableau::new(variables, &branch).solve() else { continue };
        let mut cost = Rational::zero();
        let mut fractional = None;
        for (variable, value) in vector.iter().enumerate() {
            cost = cost.add(value);
            if fractional.is_none() && !value.is_integer() {
                fractional = Some(variable);
            }
        }
        if best.as_ref().is_some_and(|(best_cost, _)| ceiling(&cost) >= *best_cost) {
            continue;
        }
        let Some(fractional) = fractional else {
            let counts: Vec<BigInt> = vector.into_iter().map(|value| value.numerator).collect();
            best = Some((cost.numerator.clone(), counts));
            continue;
        };
        let floor = &vector[fractional].numerator / &vector[fractional].denominator;
        let mut lower = branch.clone();
        lower.push(Constraint {
            indices: vec![fractional],
            coefficients: vec![BigInt::one()],
            bound: floor.clone(),
        });
        let mut upper = branch;
        upper.push(Constraint {
            indices: vec![fractional],
            coefficients: vec![-BigInt::one()],
            bound: -(&floor + BigInt::one()),
        });
        pending.push(lower);
        pending.push(upper);
    }
    match best {
        Some((_cost, counts)) => Balance::Optimal(counts),
        None => Balance::Infeasible,
    }
}

fn ceiling(value: &Rational) -> BigInt {
    let (quotient, remainder) = value.numerator.div_mod_floor(&value.denominator);
    if remainder.is_positive() { quotient + 1 } else { quotient }
}

struct Tableau {
    rows: usize,
    columns: usize,
    basic: Vec<isize>,
    nonbasic: Vec<isize>,
    dictionary: Vec<Vec<Rational>>,
}

impl Tableau {
    fn new(variables: usize, rows: &[Constraint]) -> Self {
        let m = rows.len();
        let n = variables;
        // ECO's layout. A slack's label is `n + row`, which never collides with a decision variable
        // because those occupy `0..n` and the artificial column is the one entry labelled -1.
        let mut dictionary = vec![vec![Rational::zero(); n + 2]; m + 2];
        let mut basic = vec![0isize; m];
        for (index, row) in rows.iter().enumerate() {
            for (&variable, coefficient) in row.indices.iter().zip(&row.coefficients) {
                dictionary[index][variable] = Rational::from_int(coefficient.clone());
            }
            basic[index] = (n + index) as isize;
            dictionary[index][n] = Rational::negative_one();
            dictionary[index][n + 1] = Rational::from_int(row.bound.clone());
        }
        let mut nonbasic = vec![0isize; n + 1];
        for column in 0..n {
            nonbasic[column] = column as isize;
            // Maximize -sum(x): a negative reduced cost is then the improving direction.
            dictionary[m][column] = Rational::one();
        }
        nonbasic[n] = -1;
        dictionary[m + 1][n] = Rational::one();
        Self { rows: m, columns: n, basic, nonbasic, dictionary }
    }

    fn pivot(&mut self, row: usize, column: usize) {
        // ECO's pivot, transcribed in its order. Other rows are reduced against the pivot row before
        // that row is scaled, the pivot column is then multiplied by the negated reciprocal, and the
        // pivot entry itself becomes the reciprocal.
        let inverse = Rational::one().div(&self.dictionary[row][column]);
        let width = self.columns + 2;
        for i in 0..self.rows + 2 {
            if i == row || self.dictionary[i][column].signum() == 0 {
                continue;
            }
            let factor = self.dictionary[i][column].mul(&inverse);
            for j in 0..width {
                if j != column {
                    let product = self.dictionary[row][j].mul(&factor);
                    self.dictionary[i][j] = self.dictionary[i][j].sub(&product);
                }
            }
        }
        for j in 0..width {
            if j != column {
                self.dictionary[row][j] = self.dictionary[row][j].mul(&inverse);
            }
        }
        for i in 0..self.rows + 2 {
            if i != row {
                self.dictionary[i][column] = self.dictionary[i][column].mul(&inverse).negate();
            }
        }
        self.dictionary[row][column] = inverse;
        let previous = self.basic[row];
        self.basic[row] = self.nonbasic[column];
        self.nonbasic[column] = previous;
    }

    fn simplex(&mut self, phase: u8) -> bool {
        let objective = if phase == 1 { self.rows + 1 } else { self.rows };
        let mut pivots = 0usize;
        loop {
            pivots += 1;
            // A correct Bland rule finishes within this many pivots. Passing it means the basis is
            // repeating, so the phase reports failure instead of hanging.
            if pivots > (self.rows + self.columns + 2).pow(2) {
                return false;
            }
            let mut entering = None;
            for column in 0..=self.columns {
                // Both stored rows are maximized, so a negative entry is the improving step. Phase two
                // also refuses the artificial column. Bland breaks ties by the smaller variable label.
                let improving = self.dictionary[objective][column].signum() < 0;
                if phase == 2 && self.nonbasic[column] == -1 || !improving {
                    continue;
                }
                if entering.is_none_or(|current| self.nonbasic[column] < self.nonbasic[current]) {
                    entering = Some(column);
                }
            }
            let Some(entering) = entering else { return true };
            let mut leaving: Option<usize> = None;
            for row in 0..self.rows {
                if self.dictionary[row][entering].signum() <= 0 {
                    continue;
                }
                let better: bool = match leaving {
                    None => true,
                    Some(current) => {
                        let ratio = self.dictionary[row][self.columns + 1].div(&self.dictionary[row][entering]);
                        let current_ratio = self.dictionary[current][self.columns + 1].div(&self.dictionary[current][entering]);
                        ratio.cmp_value(&current_ratio) == std::cmp::Ordering::Less
                            || ratio.cmp_value(&current_ratio) == std::cmp::Ordering::Equal && self.basic[row] < self.basic[current]
                    }
                };
                if better {
                    leaving = Some(row);
                }
            }
            // ECO treats a column with no positive entry as unbounded. The firing objective cannot be
            // unbounded for a feasible instance, so that ends the phase without a vector.
            let Some(leaving) = leaving else { return false };
            self.pivot(leaving, entering);
        }
    }

    fn solve(mut self) -> Option<Vec<Rational>> {
        let mut row = None;
        for index in 0..self.rows {
            if row.is_none_or(|current: usize| {
                self.dictionary[index][self.columns + 1].cmp_value(&self.dictionary[current][self.columns + 1]) == std::cmp::Ordering::Less
            }) {
                row = Some(index);
            }
        }
        if let Some(row) = row {
            if self.dictionary[row][self.columns + 1].signum() < 0 {
                self.pivot(row, self.columns);
                if !self.simplex(1) {
                    return None;
                }
                if self.dictionary[self.rows + 1][self.columns + 1].signum() < 0 {
                    return None;
                }
                // A nonzero phase-one value means the artificial variable never reached zero.
                if self.dictionary[self.rows + 1][self.columns + 1].signum() != 0 {
                    return None;
                }
                // Drive the artificial variable out of the basis before phase two, pivoting on the
                // eligible column with the smallest label.
                for index in 0..self.rows {
                    if self.basic[index] != -1 {
                        continue;
                    }
                    let mut column = None;
                    for candidate in 0..=self.columns {
                        if self.dictionary[index][candidate].signum() == 0 {
                            continue;
                        }
                        if column.is_none_or(|current| self.nonbasic[candidate] < self.nonbasic[current]) {
                            column = Some(candidate);
                        }
                    }
                    if let Some(column) = column {
                        self.pivot(index, column);
                    }
                }
            }
        }
        if !self.simplex(2) {
            return None;
        }
        let mut vector = vec![Rational::zero(); self.columns];
        for row in 0..self.rows {
            let basic = self.basic[row];
            if basic >= 0 && (basic as usize) < self.columns {
                vector[basic as usize] = self.dictionary[row][self.columns + 1].clone();
            }
        }
        Some(vector)
    }
}
