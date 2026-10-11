package y271727uy.stoich.api;

import y271727uy.stoich.internal.NativeSolver;

/**
 * The one entry point a caller needs. The native solver is loaded into this JVM, so ECO,
 * DataEnergistics, thunderbolt and ae2lt must depend on this jar instead of each shipping a copy.
 */
public final class Stoich {
    private Stoich() {}

    public static StoichResult solve(StoichGraph graph) {
        return NativeSolver.solve(graph);
    }
}
