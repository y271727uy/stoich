package y271727uy.stoich;

import y271727uy.stoich.api.Stoich;
import y271727uy.stoich.api.StoichGraph;
import y271727uy.stoich.api.StoichResult;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;

class StoichTest {
    @Test
    void growthIsClosedForm() {
        StoichGraph graph = StoichGraph.builder(1, 2)
                .column(0, new int[]{0, 1}, new long[]{9, 1}, new long[]{10, 0})
                .stock(0, 9)
                .target(0, 1_000_000_009L)
                .importable(1)
                .build();
        StoichResult result = Stoich.solve(graph);
        assertEquals(StoichResult.Status.SUCCESS, result.status());
        assertArrayEquals(new long[]{1_000_000_000L}, result.firings());
        assertEquals(9L, result.seed()[0]);
        assertEquals(1_000_000_000L, result.external()[1]);
        assertEquals(0L, result.states());
    }

    @Test
    void infeasibleBalanceIsProven() {
        StoichGraph graph = StoichGraph.builder(1, 2)
                .column(0, new int[]{0, 1}, new long[]{1, 0}, new long[]{0, 1})
                .target(1, 1)
                .build();
        assertEquals(StoichResult.Status.INFEASIBLE, Stoich.solve(graph).status());
    }

    @Test
    void solvesTwiceInProcess() {
        StoichGraph graph = StoichGraph.builder(1, 1)
                .column(0, new int[]{0}, new long[]{1}, new long[]{2})
                .stock(0, 1)
                .target(0, 5)
                .build();
        assertEquals(StoichResult.Status.SUCCESS, Stoich.solve(graph).status());
        assertEquals(StoichResult.Status.SUCCESS, Stoich.solve(graph).status());
    }
}
