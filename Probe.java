public class Probe {
    public static void main(String[] args) throws Exception {
        var graph = y271727uy.stoich.StoichGraph.builder(1, 2)
            .column(0, new int[]{0, 1}, new long[]{9, 1}, new long[]{10, 0})
            .stock(0, 9).target(0, 19).importable(1).build();
        try {
            var result = y271727uy.stoich.Stoich.solve(graph);
            System.out.println(result.status() + " firings=" + result.firings()[0] + " detail=" + result.detail());
        } catch (Throwable t) {
            t.printStackTrace(System.out);
        }
    }
}
