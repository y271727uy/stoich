# Bibliography

Belotti, P., Cafieri, S., Lee, J. and Liberti, L. (2012) *On feasibility-based bounds tightening*.
Available at: https://hal.science/hal-00935464v1 (Accessed: 10 October 2026).

The exact fixed point of feasibility-based bound tightening is itself a linear program. stoich does not
solve that program. Recipe coefficients are small integers, and a finite number of probes is already exact.

Commoner, F., Holt, A.W., Even, S. and Pnueli, A. (1971) 'Marked directed graphs',
*Journal of Computer and System Sciences*, 5(5), pp. 511–523.
doi: 10.1016/S0022-0000(71)80013-2.

A marked graph is live exactly when every directed circuit carries a token. A general recipe graph is
not a marked graph, so this is a certificate for that shape only.

Hillion, H.P. and Proth, J.-M. (1989) 'Performance evaluation of job-shop systems using timed event-graphs',
*IEEE Transactions on Automatic Control*, 34(1), pp. 3–9.
doi: 10.1109/9.8644.

Timed event graphs are the shape of a fixed multi-step order. The kernel uses the order, not the timing.

Laftit, S., Proth, J.-M. and Xie, X.-L. (1992) 'Optimization of invariant criteria for event graphs',
*IEEE Transactions on Automatic Control*, 37(5), pp. 547–555.
doi: 10.1109/9.135488.

The minimum initial resource of a repeated event-graph cycle is affine in the repetition count. One pass
over the cycle gives the seed for every repetition.

Savelsbergh, M.W.P. (1994) 'Preprocessing and probing techniques for mixed integer programming problems',
*ORSA Journal on Computing*, 6(4), pp. 445–454.
doi: 10.1287/ijoc.6.4.445.

The firing-count propagation. The pass limit is required: a net-consuming feedback row can raise a lower
bound without bound, and only a finite pass stays a valid substitution.

Amat, N., Dal Zilio, S. and Le Botlan, D. (2023) 'Leveraging polyhedral reductions for solving Petri net
reachability problems', *International Journal on Software Tools for Technology Transfer*, 25, pp. 33–48.
doi: 10.1007/s10009-023-00667-3.

Structural reductions that preserve reachable markings, with linear equations that map a solution on the
reduced net back to the original one. Not implemented. The reductions worth taking are the redundant
place and the redundant transition, applied before the state equation.

## Looked up and not used

Petri-net reachability, as settled by Mayr, Kosaraju and Lambert, is EXPSPACE-hard. It is not an
implementation plan. Elementary flux modes enumerate steady-state flux rays and do not answer one order.
Max-plus spectral theory gives a throughput, not a material seed.

`stock + net·x >= target` is necessary and not sufficient. A firing vector still has to be realized by a
block whose prefix requirement never goes negative.
