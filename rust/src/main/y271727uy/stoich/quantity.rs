//! Checked amounts. Recipe coefficients stay in `i64`; a block aggregate promotes itself to a
//! big integer when a product overflows, and the result reports that instead of wrapping.

use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SignedAmount {
    Small(i64),
    Big(BigInt),
}

impl SignedAmount {
    pub fn zero() -> Self {
        Self::Small(0)
    }

    pub fn from_i64(value: i64) -> Self {
        Self::Small(value)
    }

    pub fn is_zero(&self) -> bool {
        match self {
            Self::Small(value) => *value == 0,
            Self::Big(value) => value.is_zero(),
        }
    }

    pub fn signum(&self) -> i8 {
        match self {
            Self::Small(value) => value.signum() as i8,
            Self::Big(value) => match value.sign() {
                num_bigint::Sign::Minus => -1,
                num_bigint::Sign::NoSign => 0,
                num_bigint::Sign::Plus => 1,
            },
        }
    }

    pub fn fits_i64(&self) -> bool {
        self.to_i64().is_some()
    }

    pub fn to_i64(&self) -> Option<i64> {
        match self {
            Self::Small(value) => Some(*value),
            Self::Big(value) => value.to_i64(),
        }
    }

    pub fn to_bigint(&self) -> BigInt {
        match self {
            Self::Small(value) => BigInt::from(*value),
            Self::Big(value) => value.clone(),
        }
    }

    pub fn checked_add(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Small(left), Self::Small(right)) => match left.checked_add(*right) {
                Some(value) => Self::Small(value),
                None => Self::Big(BigInt::from(*left) + BigInt::from(*right)),
            },
            _ => Self::Big(self.to_bigint() + other.to_bigint()),
        }
    }

    pub fn checked_sub(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Small(left), Self::Small(right)) => match left.checked_sub(*right) {
                Some(value) => Self::Small(value),
                None => Self::Big(BigInt::from(*left) - BigInt::from(*right)),
            },
            _ => Self::Big(self.to_bigint() - other.to_bigint()),
        }
    }

    pub fn checked_mul_i64(&self, factor: i64) -> Self {
        match self {
            Self::Small(value) => match value.checked_mul(factor) {
                Some(product) => Self::Small(product),
                None => Self::Big(BigInt::from(*value) * BigInt::from(factor)),
            },
            Self::Big(value) => Self::Big(value * BigInt::from(factor)),
        }
    }

    pub fn checked_mul(&self, factor: &UnsignedAmount) -> Self {
        match factor {
            UnsignedAmount::Small(value) => self.checked_mul_i64(*value),
            UnsignedAmount::Big(value) => Self::Big(self.to_bigint() * value),
        }
    }

    pub fn max<'a>(&'a self, other: &'a Self) -> &'a Self {
        if self.cmp_amount(other).is_ge() {
            self
        } else {
            other
        }
    }

    pub fn cmp_amount(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Self::Small(left), Self::Small(right)) => left.cmp(right),
            _ => self.to_bigint().cmp(&other.to_bigint()),
        }
    }
}

impl PartialOrd for SignedAmount {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SignedAmount {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cmp_amount(other)
    }
}

impl fmt::Display for SignedAmount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Small(value) => write!(formatter, "{value}"),
            Self::Big(value) => write!(formatter, "{value}"),
        }
    }
}

/// A firing count or a repetition. Negative values are rejected at the request boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnsignedAmount {
    Small(i64),
    Big(BigInt),
}

impl UnsignedAmount {
    pub fn zero() -> Self {
        Self::Small(0)
    }

    pub fn one() -> Self {
        Self::Small(1)
    }

    pub fn try_from_i64(value: i64) -> Option<Self> {
        if value < 0 {
            None
        } else {
            Some(Self::Small(value))
        }
    }

    pub fn is_zero(&self) -> bool {
        match self {
            Self::Small(value) => *value == 0,
            Self::Big(value) => value.is_zero(),
        }
    }

    pub fn is_one(&self) -> bool {
        match self {
            Self::Small(value) => *value == 1,
            Self::Big(value) => value == &BigInt::from(1),
        }
    }

    pub fn fits_i64(&self) -> bool {
        self.to_i64().is_some()
    }

    pub fn to_i64(&self) -> Option<i64> {
        match self {
            Self::Small(value) => Some(*value),
            Self::Big(value) => value.to_i64().filter(|value| *value >= 0),
        }
    }

    pub fn to_bigint(&self) -> BigInt {
        match self {
            Self::Small(value) => BigInt::from(*value),
            Self::Big(value) => value.clone(),
        }
    }

    pub fn checked_add(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Small(left), Self::Small(right)) => match left.checked_add(*right) {
                Some(value) => Self::Small(value),
                None => Self::Big(BigInt::from(*left) + BigInt::from(*right)),
            },
            _ => Self::Big(self.to_bigint() + other.to_bigint()),
        }
    }

    pub fn saturating_sub(&self, other: &Self) -> Self {
        let difference = self.to_bigint() - other.to_bigint();
        if difference.sign() == num_bigint::Sign::Minus {
            Self::zero()
        } else {
            Self::from_bigint(difference)
        }
    }

    pub fn checked_mul(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Small(left), Self::Small(right)) => match left.checked_mul(*right) {
                Some(value) => Self::Small(value),
                None => Self::Big(BigInt::from(*left) * BigInt::from(*right)),
            },
            _ => Self::Big(self.to_bigint() * other.to_bigint()),
        }
    }

    /// Floor division. The divisor must be positive.
    pub fn div_floor(&self, divisor: &Self) -> Option<Self> {
        if divisor.is_zero() {
            return None;
        }
        Some(Self::from_bigint(self.to_bigint() / divisor.to_bigint()))
    }

    /// Ceiling division of two nonnegative amounts.
    pub fn ceil_div(&self, divisor: &Self) -> Option<Self> {
        if divisor.is_zero() {
            return None;
        }
        let quotient = self.to_bigint() / divisor.to_bigint();
        let remainder = &self.to_bigint() % &divisor.to_bigint();
        if remainder.is_zero() {
            Some(Self::from_bigint(quotient))
        } else {
            Some(Self::from_bigint(quotient + 1))
        }
    }

    pub fn min<'a>(&'a self, other: &'a Self) -> &'a Self {
        if self.cmp_amount(other).is_le() {
            self
        } else {
            other
        }
    }

    pub fn cmp_amount(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Self::Small(left), Self::Small(right)) => left.cmp(right),
            _ => self.to_bigint().cmp(&other.to_bigint()),
        }
    }

    fn from_bigint(value: BigInt) -> Self {
        match value.to_i64() {
            Some(small) if small >= 0 => Self::Small(small),
            _ => Self::Big(value),
        }
    }
}

impl PartialOrd for UnsignedAmount {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for UnsignedAmount {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cmp_amount(other)
    }
}

pub trait TryMulRep {
    fn mul_rep(self, repetitions: i64) -> Option<i64>;
}

impl TryMulRep for i64 {
    fn mul_rep(self, repetitions: i64) -> Option<i64> {
        self.checked_mul(repetitions)
    }
}
