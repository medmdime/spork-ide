/*
 * Spork IDE's test runner: runs the tests its arguments select with the framework the project
 * uses (JUnit 4, the JUnit Platform for JUnit 5 and 6, or TestNG) and reports their events to the
 * IDE, which draws its test tree from them, as IntelliJ's own runners do. GPL-3.0-or-later.
 *
 * usage: java -cp <this>:<the test classpath> spork.tests.TestRunner <selector>...
 *    or: java -cp <this> spork.tests.TestRunner --classpath <the test classpath> <selector>...
 *   class:<class>  method:<class>#<method>  package:<package>  pattern:<regex of class names>
 * The events go to the loopback port in SPORK_TEST_PORT, one service message a line
 * (##teamcity[testStarted nodeId='2' parentNodeId='1' name='…' locationHint='java:test://…']);
 * the tests' output goes to the console as written and to the IDE as their nodes' output.
 */
package spork.tests;

import java.io.File;
import java.io.IOException;
import java.io.OutputStream;
import java.io.PrintStream;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.lang.annotation.Annotation;
import java.lang.reflect.Method;
import java.net.Socket;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

public final class TestRunner {
    private TestRunner() {}

    public static void main(String[] args) throws Exception {
        if (args.length > 1 && args[0].equals("--classpath")) {
            // a classpath given apart (Maven exec plugin's %classpath): the runner and the tests
            // in a class loader of their own
            List<URL> urls = new ArrayList<>();
            urls.add(TestRunner.class.getProtectionDomain().getCodeSource().getLocation());
            for (String entry : args[1].split(File.pathSeparator)) {
                if (!entry.isEmpty()) {
                    urls.add(new File(entry).toURI().toURL());
                }
            }
            ClassLoader loader = new URLClassLoader(urls.toArray(new URL[0]), ClassLoader.getPlatformClassLoader());
            Thread.currentThread().setContextClassLoader(loader);
            System.setProperty("java.class.path", args[1]);
            Class.forName(TestRunner.class.getName(), true, loader)
                    .getMethod("main", String[].class)
                    .invoke(null, (Object) Arrays.copyOfRange(args, 2, args.length));
            return;
        }
        List<String> selectors = new ArrayList<>();
        for (String arg : args) {
            if (!arg.isEmpty()) {
                selectors.add(arg);
            }
        }
        Events events = Events.connect(System.getenv("SPORK_TEST_PORT"));
        System.setOut(new PrintStream(new Tee(System.out, events, false), true, "UTF-8"));
        System.setErr(new PrintStream(new Tee(System.err, events, true), true, "UTF-8"));
        boolean passed;
        try {
            switch (framework(selectors)) {
                case "testng":
                    passed = TestNgTests.run(selectors, events);
                    break;
                case "platform":
                    passed = PlatformTests.run(selectors, events);
                    break;
                default:
                    passed = JUnit4Tests.run(selectors, events);
            }
        } finally {
            events.close();
        }
        System.exit(passed ? 0 : 1);
    }

    /** The framework of the selected classes: the annotations of their methods decide. */
    static String framework(List<String> selectors) {
        for (String selector : selectors) {
            String kind = selector.substring(0, Math.max(selector.indexOf(':'), 0));
            if (!kind.equals("class") && !kind.equals("method")) {
                continue;
            }
            String name = className(selector);
            Class<?> type = load(name);
            String found = type == null ? null : frameworkOf(type);
            if (found != null) {
                return found;
            }
        }
        if (present("org.junit.platform.launcher.core.LauncherFactory")
                && present("org.junit.jupiter.api.Test")) {
            return "platform";
        }
        if (!present("org.junit.Test") && present("org.testng.TestNG")) {
            return "testng";
        }
        return present("org.junit.Test") ? "junit4" : "platform";
    }

    static String frameworkOf(Class<?> type) {
        for (Class<?> c = type; c != null && c != Object.class; c = c.getSuperclass()) {
            for (Method method : c.getDeclaredMethods()) {
                for (Annotation annotation : method.getAnnotations()) {
                    String name = annotation.annotationType().getName();
                    if (name.startsWith("org.junit.jupiter.")) {
                        return "platform";
                    }
                    if (name.startsWith("org.testng.")) {
                        return "testng";
                    }
                    if (name.equals("org.junit.Test")) {
                        return "junit4";
                    }
                }
            }
            for (Annotation annotation : c.getAnnotations()) {
                String name = annotation.annotationType().getName();
                if (name.startsWith("org.testng.")) {
                    return "testng";
                }
                if (name.equals("org.junit.runner.RunWith")) {
                    return "junit4";
                }
            }
        }
        return null;
    }

    static boolean present(String name) {
        return load(name) != null;
    }

    static Class<?> load(String name) {
        try {
            return Class.forName(name, false, TestRunner.class.getClassLoader());
        } catch (ClassNotFoundException | LinkageError e) {
            return null;
        }
    }

    /** The class a `class:` or `method:` selector names. */
    static String className(String selector) {
        String rest = selector.substring(selector.indexOf(':') + 1);
        int hash = rest.indexOf('#');
        return hash < 0 ? rest : rest.substring(0, hash);
    }

    static String methodName(String selector) {
        int hash = selector.indexOf('#');
        return hash < 0 ? null : selector.substring(hash + 1);
    }

    static String value(String selector) {
        return selector.substring(selector.indexOf(':') + 1);
    }

    /** The classes of the classpath's folders (the compiled sources) a package or pattern selects. */
    static List<Class<?>> classesIn(List<String> selectors) {
        List<String> names = new ArrayList<>();
        for (String entry : System.getProperty("java.class.path").split(File.pathSeparator)) {
            File root = new File(entry);
            if (root.isDirectory()) {
                collect(root, "", names);
            }
        }
        List<Class<?>> classes = new ArrayList<>();
        for (String name : names) {
            if (name.contains("$") || !selected(name, selectors)) {
                continue;
            }
            Class<?> type = load(name);
            if (type != null && !type.getName().startsWith("spork.tests.")) {
                classes.add(type);
            }
        }
        return classes;
    }

    static boolean selected(String className, List<String> selectors) {
        for (String selector : selectors) {
            if (selector.startsWith("package:")) {
                String pkg = value(selector);
                if (pkg.isEmpty() || className.startsWith(pkg + ".")) {
                    return true;
                }
            } else if (selector.startsWith("pattern:")
                    && Pattern.compile(value(selector)).matcher(className).find()) {
                return true;
            }
        }
        return false;
    }

    private static void collect(File folder, String prefix, List<String> names) {
        File[] files = folder.listFiles();
        if (files == null) {
            return;
        }
        for (File file : files) {
            String name = file.getName();
            if (file.isDirectory()) {
                collect(file, prefix + name + ".", names);
            } else if (name.endsWith(".class")) {
                names.add(prefix + name.substring(0, name.length() - ".class".length()));
            }
        }
    }

    /** The failure's stack trace, up to the frames that called the test. */
    static String trace(Throwable failure) {
        StringWriter text = new StringWriter();
        failure.printStackTrace(new PrintWriter(text));
        StringBuilder kept = new StringBuilder();
        for (String line : text.toString().split("\n")) {
            String trimmed = line.trim();
            if (trimmed.startsWith("at ") && callsTests(trimmed.substring(3))) {
                break;
            }
            kept.append(line.replace("\r", "")).append('\n');
        }
        return kept.toString();
    }

    private static boolean callsTests(String frame) {
        String[] callers = {
            "java.base/jdk.internal.reflect.", "jdk.internal.reflect.", "sun.reflect.",
            "java.base/java.lang.reflect.", "java.lang.reflect.", "org.junit.runners.",
            "org.junit.platform.", "org.testng.internal.", "spork.tests."
        };
        for (String caller : callers) {
            if (frame.startsWith(caller)) {
                return true;
            }
        }
        return false;
    }

    private static final Pattern[] COMPARISONS = {
        Pattern.compile("expected:\\s*<(.*)> but was:\\s*<(.*)>", Pattern.DOTALL),
        Pattern.compile("expected \\[(.*)\\] but found \\[(.*)\\]", Pattern.DOTALL),
    };

    /** A comparison failure's expected and actual values, as IntelliJ's diff shows them. */
    static String[] comparison(Throwable failure) {
        try {
            Method defined = failure.getClass().getMethod("isExpectedDefined");
            if (Boolean.TRUE.equals(defined.invoke(failure))
                    && Boolean.TRUE.equals(failure.getClass().getMethod("isActualDefined").invoke(failure))) {
                return new String[] {valueText(failure, "getExpected"), valueText(failure, "getActual")};
            }
        } catch (ReflectiveOperationException e) {
            // not opentest4j's
        }
        for (Class<?> c = failure.getClass(); c != null; c = c.getSuperclass()) {
            if (c.getName().endsWith(".ComparisonFailure")) {
                try {
                    return new String[] {
                        String.valueOf(c.getMethod("getExpected").invoke(failure)),
                        String.valueOf(c.getMethod("getActual").invoke(failure))
                    };
                } catch (ReflectiveOperationException e) {
                    break;
                }
            }
        }
        String message = failure.getMessage();
        if (message != null && failure instanceof AssertionError) {
            for (Pattern pattern : COMPARISONS) {
                Matcher matcher = pattern.matcher(message);
                if (matcher.find()) {
                    return new String[] {matcher.group(1), matcher.group(2)};
                }
            }
        }
        return null;
    }

    private static String valueText(Throwable failure, String getter) throws ReflectiveOperationException {
        Object value = failure.getClass().getMethod(getter).invoke(failure);
        if (value == null) {
            return "null";
        }
        try {
            return String.valueOf(value.getClass().getMethod("getStringRepresentation").invoke(value));
        } catch (NoSuchMethodException e) {
            return String.valueOf(value);
        }
    }

    /** The service messages to the IDE; nothing without its port. */
    static final class Events {
        private final OutputStream out;
        private final AtomicInteger ids = new AtomicInteger();
        /** The test whose output the console shows now. */
        volatile String current;

        private Events(OutputStream out) {
            this.out = out;
        }

        static Events connect(String port) {
            if (port == null || port.isEmpty()) {
                return new Events(null);
            }
            try {
                @SuppressWarnings("resource")
                Socket socket = new Socket("127.0.0.1", Integer.parseInt(port.trim()));
                return new Events(socket.getOutputStream());
            } catch (IOException | NumberFormatException e) {
                return new Events(null);
            }
        }

        String nextId() {
            return Integer.toString(ids.incrementAndGet());
        }

        synchronized void send(String name, String... attributes) {
            if (out == null) {
                return;
            }
            StringBuilder line = new StringBuilder("##teamcity[").append(name);
            for (int i = 0; i + 1 < attributes.length; i += 2) {
                if (attributes[i + 1] == null) {
                    continue;
                }
                line.append(' ').append(attributes[i]).append("='").append(escape(attributes[i + 1])).append('\'');
            }
            line.append("]\n");
            try {
                out.write(line.toString().getBytes(StandardCharsets.UTF_8));
                out.flush();
            } catch (IOException e) {
                // the IDE went away: the run goes on in its console
            }
        }

        void suiteStarted(String id, String parent, String name, String className) {
            send("testSuiteStarted", "nodeId", id, "parentNodeId", parent, "name", name,
                    "locationHint", className == null ? null : "java:suite://" + className);
        }

        void suiteFinished(String id) {
            send("testSuiteFinished", "nodeId", id);
        }

        void testStarted(String id, String parent, String name, String className, String method) {
            current = id;
            send("testStarted", "nodeId", id, "parentNodeId", parent, "name", name,
                    "locationHint", "java:test://" + className + "/" + method);
        }

        void testFinished(String id, long millis) {
            send("testFinished", "nodeId", id, "duration", Long.toString(millis));
            current = null;
        }

        void testFailed(String id, Throwable failure) {
            String[] comparison = comparison(failure);
            String message = failure.getMessage();
            send("testFailed", "nodeId", id, "message", message == null ? "" : message,
                    "details", trace(failure),
                    "type", comparison == null ? null : "comparisonFailure",
                    "expected", comparison == null ? null : comparison[0],
                    "actual", comparison == null ? null : comparison[1]);
        }

        void testIgnored(String id, String message) {
            send("testIgnored", "nodeId", id, "message", message == null ? "" : message);
        }

        void count(int count) {
            send("testCount", "count", Integer.toString(count));
        }

        void output(String text, boolean error) {
            String id = current;
            if (id != null) {
                send(error ? "testStdErr" : "testStdOut", "nodeId", id, "out", text);
            }
        }

        void close() {
            if (out != null) {
                try {
                    out.close();
                } catch (IOException e) {
                    // closed already
                }
            }
        }

        static String escape(String text) {
            StringBuilder escaped = new StringBuilder(text.length());
            for (char c : text.toCharArray()) {
                switch (c) {
                    case '\'': escaped.append("|'"); break;
                    case '\n': escaped.append("|n"); break;
                    case '\r': escaped.append("|r"); break;
                    case '|': escaped.append("||"); break;
                    case '[': escaped.append("|["); break;
                    case ']': escaped.append("|]"); break;
                    default: escaped.append(c);
                }
            }
            return escaped.toString();
        }
    }

    /** The console's stream, also sent to the IDE as the running test's output. */
    static final class Tee extends OutputStream {
        private final PrintStream console;
        private final Events events;
        private final boolean error;

        Tee(PrintStream console, Events events, boolean error) {
            this.console = console;
            this.events = events;
            this.error = error;
        }

        @Override
        public void write(int b) {
            write(new byte[] {(byte) b}, 0, 1);
        }

        @Override
        public void write(byte[] bytes, int offset, int length) {
            console.write(bytes, offset, length);
            events.output(new String(bytes, offset, length, StandardCharsets.UTF_8), error);
        }

        @Override
        public void flush() {
            console.flush();
        }
    }
}
