package y271727uy.stoich.api;

/**
 * Outcome of one graph. Amounts use the caller's {@code long} range; a value that does not fit comes
 * back as {@link Status#UNREPRESENTABLE} rather than a wrapped number.
 */
public record StoichResult(
        Status status,
        long[] firings,
        long[] seed,
        long[] external,
        long[] shortfall,
        Step[] steps,
        Interval[] intervals,
        long states,
        String detail,
        /**
         * Pareto frontier of the internal input each recipe lacks before its first firing. Empty unless
         * the status is a proven shortage. Each array is one candidate, aligned to the caller's items.
         */
        long[][] startup
) {
    public enum Status {
        SUCCESS,
        /**
         * Proven at this stock. Either material balance has no solution, or every reachable marking
         * was expanded and none meets the target. This is ECO's {@code INSUFFICIENT_EXTERNAL_INPUT}.
         */
        INSUFFICIENT_SEED,
        INFEASIBLE,
        /** The marking budget or the firing depth closed the search early. Nothing is proven. */
        BUDGET_EXHAUSTED,
        UNREPRESENTABLE,
        /** Retained for a worker that still reports an open interval. New results use the two proofs above. */
        NOT_CLOSED,
        REJECTED
    }

    public record Step(int recipe, long count, int repeatWidth, long repetitions) {}

    public record Interval(long lower, boolean hasUpper, long upper) {}

    public boolean solved() {
        return status == Status.SUCCESS;
    }

    /** True when the answer is a proof, not a budget cut. Matches ECO's {@code proven()}. */
    public boolean proven() {
        return status == Status.SUCCESS || status == Status.INSUFFICIENT_SEED || status == Status.INFEASIBLE;
    }
}
