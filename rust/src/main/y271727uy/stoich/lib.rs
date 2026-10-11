//! Exact recipe stoichiometry shared by ECO, DataEnergistics, thunderbolt and ae2lt.
//!
//! The object every caller shares is a block: the prefix requirement and net effect of an
//! arbitrary firing sequence, composed and repeated in closed form. Replay therefore costs
//! the number of blocks, never the number of firings.

mod balance;
mod bounds;
mod quantity;
mod shape;
mod solve;

pub use balance::{minimize, Balance};
pub use bounds::{propagate, Bounds, FiringInterval};
pub use quantity::{SignedAmount, TryMulRep, UnsignedAmount};
pub use shape::{fixed_order, single_growth, Shape};
pub use solve::{solve, SolveRequest, SolveResult, Status, Step, MAX_DEPTH};

#[cfg(test)]
#[path = "../../../test/y271727uy/stoich/tests.rs"]
mod tests;
