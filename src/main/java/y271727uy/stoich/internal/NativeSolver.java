package y271727uy.stoich.internal;

import y271727uy.stoich.api.StoichGraph;
import y271727uy.stoich.api.StoichResult;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;

/**
 * Loads the solver into this JVM and calls it. The library is extracted once to a versioned file
 * under the working directory and reused; later calls do not unpack another copy.
 *
 * <p>A native fault takes this process down. That is accepted: Java faulting does the same.
 */
public final class NativeSolver {
    static final String VERSION = "0.1.0";

    private static volatile boolean loaded;

    private NativeSolver() {}

    public static StoichResult solve(StoichGraph graph) {
        load();
        byte[] response = solve0(Proto.encodeRequest(graph, 0));
        return toResult(Proto.decodeResponse(response));
    }

    private static void load() {
        if (loaded) {
            return;
        }
        synchronized (NativeSolver.class) {
            if (loaded) {
                return;
            }
            try {
                System.load(library().toAbsolutePath().toString());
            } catch (IOException failure) {
                throw new IllegalStateException("stoich native library could not be extracted", failure);
            } catch (UnsatisfiedLinkError failure) {
                throw new IllegalStateException("stoich native library could not be loaded", failure);
            }
            loaded = true;
        }
    }

    private static Path library() throws IOException {
        String override = System.getenv("STOICH_LIBRARY");
        if (override != null && !override.isBlank()) {
            return Path.of(override);
        }
        String resource = "/native/" + platform() + "/" + libraryName(VERSION);
        // Android reports os.name as Linux, and its working directory is not writable.
        String base = android() ? System.getProperty("java.io.tmpdir", ".") : System.getProperty("user.dir", ".");
        Path directory = Path.of(base, ".stoich");
        Files.createDirectories(directory);
        Path destination = directory.resolve(libraryName(VERSION));
        if (Files.exists(destination)) {
            return destination;
        }
        try (InputStream source = NativeSolver.class.getResourceAsStream(resource)) {
            if (source == null) {
                throw new IOException("this jar has no solver library for " + platform());
            }
            Path temporary = Files.createTempFile(directory, "stoich-", ".partial");
            Files.copy(source, temporary, StandardCopyOption.REPLACE_EXISTING);
            try {
                Files.move(temporary, destination, StandardCopyOption.ATOMIC_MOVE);
            } catch (IOException raced) {
                Files.deleteIfExists(temporary);
                if (!Files.exists(destination)) {
                    throw raced;
                }
            }
        }
        return destination;
    }

    private static String platform() {
        if (android()) {
            return "android-" + androidAbi();
        }
        String os = System.getProperty("os.name", "").toLowerCase();
        String arch = System.getProperty("os.arch", "").toLowerCase();
        String family = os.contains("win") ? "windows" : os.contains("mac") ? "macos" : "linux";
        String architecture = arch.contains("aarch64") || arch.contains("arm64") ? "aarch64" : "x86_64";
        return family + "-" + architecture;
    }

    private static boolean android() {
        String vendor = System.getProperty("java.vendor", "");
        String specification = System.getProperty("java.specification.vendor", "");
        return vendor.contains("Android") || specification.contains("Android");
    }

    private static String androidAbi() {
        String arch = System.getProperty("os.arch", "").toLowerCase();
        if (arch.contains("aarch64") || arch.contains("arm64")) {
            return "arm64-v8a";
        }
        if (arch.contains("arm")) {
            return "armeabi-v7a";
        }
        if (arch.contains("x86_64") || arch.contains("amd64")) {
            return "x86_64";
        }
        return "x86";
    }

    private static String libraryName(String version) {
        String os = System.getProperty("os.name", "").toLowerCase();
        if (os.contains("win")) {
            return "stoich-" + version + ".dll";
        }
        if (os.contains("mac")) {
            return "libstoich-" + version + ".dylib";
        }
        return "libstoich-" + version + ".so";
    }

    private static native byte[] solve0(byte[] request);

    private static StoichResult toResult(Proto.Response response) {
        StoichResult.Status status = switch (response.status()) {
            case 1 -> StoichResult.Status.SUCCESS;
            case 2 -> StoichResult.Status.INSUFFICIENT_SEED;
            case 3 -> StoichResult.Status.INFEASIBLE;
            case 4 -> StoichResult.Status.BUDGET_EXHAUSTED;
            case 5 -> StoichResult.Status.UNREPRESENTABLE;
            case 6 -> StoichResult.Status.NOT_CLOSED;
            default -> StoichResult.Status.REJECTED;
        };
        StoichResult.Step[] steps = new StoichResult.Step[response.steps().length];
        for (int i = 0; i < steps.length; i++) {
            Proto.Response.Step step = response.steps()[i];
            steps[i] = new StoichResult.Step(step.recipe(), step.count(), step.repeatWidth(), step.repetitions());
        }
        StoichResult.Interval[] intervals = new StoichResult.Interval[response.intervals().length];
        for (int i = 0; i < intervals.length; i++) {
            Proto.Response.Interval interval = response.intervals()[i];
            intervals[i] = new StoichResult.Interval(interval.lower(), interval.hasUpper(), interval.upper());
        }
        return new StoichResult(status, response.firings(), response.seed(), response.external(), response.shortfall(),
                steps, intervals, response.states(), response.detail(), response.startup());
    }
}
