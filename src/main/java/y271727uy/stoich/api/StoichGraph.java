package y271727uy.stoich.api;

import java.util.Arrays;

/**
 * A recipe graph whose items are already numbered. Columns are sparse: a missing item is zero.
 * Nothing in this type is an AE key, a string name, or a Minecraft object.
 */
public record StoichGraph(
        int recipes,
        int items,
        int[] columnStart,
        int[] itemIndex,
        long[] consumed,
        long[] produced,
        long[] stock,
        long[] target,
        boolean[] importable,
        long[] candidate,
        int[] orderRecipe,
        long[] orderCount,
        Integer targetItem,
        long maxStates
) {
    public StoichGraph {
        if (recipes < 0 || items < 0) {
            throw new IllegalArgumentException("recipe and item counts must be nonnegative");
        }
        if (columnStart.length != recipes + 1) {
            throw new IllegalArgumentException("column starts need one more entry than the recipe count");
        }
        if (itemIndex.length != consumed.length || consumed.length != produced.length) {
            throw new IllegalArgumentException("sparse index, consumption and production differ in length");
        }
        if (stock.length != items || target.length != items || importable.length != items) {
            throw new IllegalArgumentException("stock, target and import mask need one entry per item");
        }
        if (candidate != null && candidate.length != recipes) {
            throw new IllegalArgumentException("a candidate firing vector needs one entry per recipe");
        }
        if ((orderRecipe == null) != (orderCount == null) || orderRecipe != null && orderRecipe.length != orderCount.length) {
            throw new IllegalArgumentException("a fixed order needs paired recipe and count arrays");
        }
        if (maxStates <= 0) {
            throw new IllegalArgumentException("the state budget must be positive");
        }
        columnStart = columnStart.clone();
        itemIndex = itemIndex.clone();
        consumed = consumed.clone();
        produced = produced.clone();
        stock = stock.clone();
        target = target.clone();
        importable = importable.clone();
        candidate = candidate == null ? null : candidate.clone();
        orderRecipe = orderRecipe == null ? null : orderRecipe.clone();
        orderCount = orderCount == null ? null : orderCount.clone();
    }

    public static Builder builder(int recipes, int items) {
        return new Builder(recipes, items);
    }

    public static final class Builder {
        private final int recipes;
        private final int items;
        private final int[] columnStart;
        private int[] itemIndex = new int[8];
        private long[] consumed = new long[8];
        private long[] produced = new long[8];
        private int entries;
        private final long[] stock;
        private final long[] target;
        private final boolean[] importable;
        private long[] candidate;
        private int[] orderRecipe;
        private long[] orderCount;
        private Integer targetItem;
        private long maxStates = 250_000L;

        private Builder(int recipes, int items) {
            this.recipes = recipes;
            this.items = items;
            this.columnStart = new int[recipes + 1];
            this.stock = new long[items];
            this.target = new long[items];
            this.importable = new boolean[items];
        }

        public Builder column(int recipe, int[] items, long[] consumed, long[] produced) {
            if (recipe < 0 || recipe >= recipes) {
                throw new IllegalArgumentException("recipe " + recipe + " is outside the graph");
            }
            ensure(items.length);
            for (int i = 0; i < items.length; i++) {
                itemIndex[entries] = items[i];
                this.consumed[entries] = consumed[i];
                this.produced[entries] = produced[i];
                entries++;
            }
            columnStart[recipe + 1] = entries;
            return this;
        }

        public Builder stock(int item, long amount) {
            stock[item] = amount;
            return this;
        }

        public Builder target(int item, long amount) {
            target[item] = amount;
            return this;
        }

        public Builder importable(int item) {
            importable[item] = true;
            return this;
        }

        public Builder candidate(long[] candidate) {
            this.candidate = candidate;
            return this;
        }

        public Builder order(int[] recipes, long[] counts, int targetItem) {
            this.orderRecipe = recipes;
            this.orderCount = counts;
            this.targetItem = targetItem;
            return this;
        }

        public Builder maxStates(long maxStates) {
            this.maxStates = maxStates;
            return this;
        }

        public StoichGraph build() {
            for (int recipe = 1; recipe < columnStart.length; recipe++) {
                if (columnStart[recipe] < columnStart[recipe - 1]) {
                    columnStart[recipe] = columnStart[recipe - 1];
                }
            }
            columnStart[recipes] = entries;
            return new StoichGraph(recipes, items, columnStart,
                    Arrays.copyOf(itemIndex, entries), Arrays.copyOf(consumed, entries), Arrays.copyOf(produced, entries), stock, target,
                    importable, candidate, orderRecipe, orderCount, targetItem, maxStates);
        }

        private void ensure(int count) {
            if (entries + count <= itemIndex.length) {
                return;
            }
            int grown = itemIndex.length;
            while (grown < entries + count) {
                grown *= 2;
            }
            itemIndex = Arrays.copyOf(itemIndex, grown);
            consumed = Arrays.copyOf(consumed, grown);
            produced = Arrays.copyOf(produced, grown);
        }
    }
}
