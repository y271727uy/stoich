package y271727uy.stoich.internal;

import java.util.Arrays;

/**
 * The subset of protobuf encoding this protocol uses. Generated classes stay out of the public API,
 * and this codec is package-private for the same reason.
 */
final class Proto {
    static final int PROTOCOL_VERSION = 1;

    private Proto() {}

    static byte[] encodeRequest(y271727uy.stoich.api.StoichGraph graph, long requestId) {
        Buffer buffer = new Buffer();
        buffer.varint(1, requestId);
        buffer.varint(2, graph.recipes());
        buffer.varint(3, graph.items());
        buffer.packedInt(4, graph.columnStart());
        buffer.packedInt(5, graph.itemIndex());
        buffer.packedLong(6, graph.consumed());
        buffer.packedLong(7, graph.produced());
        buffer.packedLong(8, graph.stock());
        buffer.packedLong(9, graph.target());
        buffer.bytes(10, importMask(graph.importable()));
        if (graph.candidate() != null) {
            buffer.bool(11, true);
            buffer.packedLong(12, graph.candidate());
        }
        if (graph.orderRecipe() != null) {
            buffer.bool(13, true);
            buffer.packedInt(14, graph.orderRecipe());
            buffer.packedLong(15, graph.orderCount());
        }
        if (graph.targetItem() != null) {
            buffer.bool(16, true);
            buffer.varint(17, graph.targetItem());
        }
        buffer.varint(18, graph.maxStates());
        return buffer.toByteArray();
    }

    static Response decodeResponse(byte[] message) {
        return Response.read(message);
    }

    private static byte[] importMask(boolean[] importable) {
        byte[] mask = new byte[(importable.length + 7) / 8];
        for (int item = 0; item < importable.length; item++) {
            if (importable[item]) {
                mask[item / 8] |= (byte) (1 << (item % 8));
            }
        }
        return mask;
    }

    record Response(
            long requestId,
            int status,
            long[] firings,
            long[] seed,
            long[] external,
            long[] shortfall,
            Step[] steps,
            Interval[] intervals,
            long states,
            String detail,
            long[][] startup
    ) {
        record Step(int recipe, long count, int repeatWidth, long repetitions) {}

        record Interval(long lower, boolean hasUpper, long upper) {}

        static Response read(byte[] message) {
            Reader reader = new Reader(message);
            long requestId = 0;
            int status = 0;
            long[] firings = new long[0];
            long[] seed = new long[0];
            long[] external = new long[0];
            long[] shortfall = new long[0];
            Step[] steps = new Step[4];
            int stepCount = 0;
            Interval[] intervals = new Interval[4];
            int intervalCount = 0;
            long states = 0;
            String detail = "";
            long[][] startup = new long[4][];
            int startupCount = 0;
            while (reader.hasMore()) {
                int tag = (int) reader.varint();
                int field = tag >>> 3;
                int wire = tag & 7;
                switch (field) {
                    case 1 -> requestId = reader.varint();
                    case 2 -> status = (int) reader.varint();
                    case 3 -> firings = reader.packedLong();
                    case 4 -> seed = reader.packedLong();
                    case 5 -> external = reader.packedLong();
                    case 6 -> shortfall = reader.packedLong();
                    case 7 -> {
                        if (stepCount == steps.length) {
                            steps = Arrays.copyOf(steps, stepCount * 2);
                        }
                        steps[stepCount++] = StepCodec.read(reader.bytes());
                    }
                    case 8 -> {
                        if (intervalCount == intervals.length) {
                            intervals = Arrays.copyOf(intervals, intervalCount * 2);
                        }
                        intervals[intervalCount++] = IntervalCodec.read(reader.bytes());
                    }
                    case 9 -> states = reader.varint();
                    case 10 -> detail = new String(reader.bytes(), java.nio.charset.StandardCharsets.UTF_8);
                    case 11 -> {
                        if (startupCount == startup.length) {
                            startup = Arrays.copyOf(startup, startupCount * 2);
                        }
                        startup[startupCount++] = startupMissing(reader.bytes());
                    }
                    default -> reader.skip(wire);
                }
            }
            return new Response(requestId, status, firings, seed, external, shortfall,
                    Arrays.copyOf(steps, stepCount), Arrays.copyOf(intervals, intervalCount),
                    states, detail, Arrays.copyOf(startup, startupCount));
        }

        /** One startup candidate is a length-delimited message holding a single packed {@code missing} field. */
        private static long[] startupMissing(byte[] message) {
            Reader reader = new Reader(message);
            long[] missing = new long[0];
            while (reader.hasMore()) {
                int tag = (int) reader.varint();
                int field = tag >>> 3;
                int wire = tag & 7;
                if (field == 1) {
                    missing = reader.packedLong();
                } else {
                    reader.skip(wire);
                }
            }
            return missing;
        }
    }

    private static final class StepCodec {
        static Response.Step read(byte[] message) {
            Reader reader = new Reader(message);
            int recipe = 0;
            long count = 0;
            int width = 0;
            long repetitions = 0;
            while (reader.hasMore()) {
                int tag = (int) reader.varint();
                switch (tag >>> 3) {
                    case 1 -> recipe = (int) reader.varint();
                    case 2 -> count = reader.signed();
                    case 3 -> width = (int) reader.varint();
                    case 4 -> repetitions = reader.signed();
                    default -> reader.skip(tag & 7);
                }
            }
            return new Response.Step(recipe, count, width, repetitions);
        }
    }

    private static final class IntervalCodec {
        static Response.Interval read(byte[] message) {
            Reader reader = new Reader(message);
            long lower = 0;
            boolean hasUpper = false;
            long upper = 0;
            while (reader.hasMore()) {
                int tag = (int) reader.varint();
                switch (tag >>> 3) {
                    case 1 -> lower = reader.signed();
                    case 2 -> hasUpper = reader.varint() != 0;
                    case 3 -> upper = reader.signed();
                    default -> reader.skip(tag & 7);
                }
            }
            return new Response.Interval(lower, hasUpper, upper);
        }
    }

    /**
     * One flat byte buffer. Packed fields are written inline with their length patched afterwards, so
     * a request costs a single array and one copy at the end instead of a stream per field.
     */
    private static final class Buffer {
        private byte[] data = new byte[64];
        private int size;

        void varint(int field, long value) {
            tag(field, 0);
            writeVarint(value);
        }

        void bool(int field, boolean value) {
            varint(field, value ? 1 : 0);
        }

        void bytes(int field, byte[] value) {
            tag(field, 2);
            writeVarint(value.length);
            write(value, 0, value.length);
        }

        void packedInt(int field, int[] values) {
            int start = packedStart(field);
            for (int value : values) {
                writeVarint(value & 0xffffffffL);
            }
            packedEnd(start);
        }

        void packedLong(int field, long[] values) {
            int start = packedStart(field);
            for (long value : values) {
                // proto3 int64 is a plain varint. sint64 would be zigzag; this protocol is not.
                writeVarint(value);
            }
            packedEnd(start);
        }

        byte[] toByteArray() {
            return Arrays.copyOf(data, size);
        }

        /** Reserve one length byte, which covers every packed field up to 127 bytes. */
        private int packedStart(int field) {
            tag(field, 2);
            ensure(1);
            int lengthAt = size;
            data[size++] = 0;
            return lengthAt;
        }

        private void packedEnd(int lengthAt) {
            int length = size - lengthAt - 1;
            if (length <= 127) {
                data[lengthAt] = (byte) length;
                return;
            }
            // The payload outgrew the reserved byte. Shift it up and write the full varint length.
            int width = varintWidth(length);
            ensure(width - 1);
            System.arraycopy(data, lengthAt + 1, data, lengthAt + width, length);
            size += width - 1;
            writeVarintAt(lengthAt, length);
        }

        private void tag(int field, int wire) {
            writeVarint((field << 3) | wire);
        }

        private void writeVarint(long value) {
            ensure(10);
            while ((value & ~0x7fL) != 0) {
                data[size++] = (byte) ((value & 0x7f) | 0x80);
                value >>>= 7;
            }
            data[size++] = (byte) value;
        }

        private void writeVarintAt(int at, long value) {
            while ((value & ~0x7fL) != 0) {
                data[at++] = (byte) ((value & 0x7f) | 0x80);
                value >>>= 7;
            }
            data[at] = (byte) value;
        }

        private void write(byte[] value, int offset, int length) {
            ensure(length);
            System.arraycopy(value, offset, data, size, length);
            size += length;
        }

        private void ensure(int count) {
            if (size + count <= data.length) {
                return;
            }
            int grown = data.length;
            while (grown < size + count) {
                grown *= 2;
            }
            data = Arrays.copyOf(data, grown);
        }

        private static int varintWidth(long value) {
            int width = 1;
            while ((value & ~0x7fL) != 0) {
                width++;
                value >>>= 7;
            }
            return width;
        }
    }

    private static final class Reader {
        private final byte[] data;
        private int cursor;

        Reader(byte[] data) {
            this.data = data;
        }

        boolean hasMore() {
            return cursor < data.length;
        }

        long varint() {
            long result = 0;
            int shift = 0;
            while (true) {
                int piece = data[cursor++] & 0xff;
                result |= (long) (piece & 0x7f) << shift;
                if ((piece & 0x80) == 0) {
                    return result;
                }
                shift += 7;
            }
        }

        /** A proto3 int64. The varint already has the two's complement width. */
        long signed() {
            return varint();
        }

        byte[] bytes() {
            int length = (int) varint();
            byte[] value = Arrays.copyOfRange(data, cursor, cursor + length);
            cursor += length;
            return value;
        }

        long[] packedLong() {
            byte[] packed = bytes();
            Reader reader = new Reader(packed);
            long[] values = new long[8];
            int count = 0;
            while (reader.hasMore()) {
                if (count == values.length) {
                    values = Arrays.copyOf(values, count * 2);
                }
                values[count++] = reader.signed();
            }
            return Arrays.copyOf(values, count);
        }

        void skip(int wire) {
            switch (wire) {
                case 0 -> varint();
                case 1 -> cursor += 8;
                case 2 -> cursor += (int) varint();
                case 5 -> cursor += 4;
                default -> throw new IllegalStateException("unsupported protobuf wire type " + wire);
            }
        }
    }
}
