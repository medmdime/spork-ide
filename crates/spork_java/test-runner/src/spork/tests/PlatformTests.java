/* Spork IDE's test runner, the JUnit Platform's part (JUnit 5 and 6). GPL-3.0-or-later. */
package spork.tests;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

import org.junit.platform.engine.DiscoverySelector;
import org.junit.platform.engine.TestExecutionResult;
import org.junit.platform.engine.TestSource;
import org.junit.platform.engine.discovery.ClassNameFilter;
import org.junit.platform.engine.discovery.DiscoverySelectors;
import org.junit.platform.engine.support.descriptor.ClassSource;
import org.junit.platform.engine.support.descriptor.MethodSource;
import org.junit.platform.launcher.Launcher;
import org.junit.platform.launcher.LauncherDiscoveryRequest;
import org.junit.platform.launcher.TestExecutionListener;
import org.junit.platform.launcher.TestIdentifier;
import org.junit.platform.launcher.TestPlan;
import org.junit.platform.launcher.core.LauncherDiscoveryRequestBuilder;
import org.junit.platform.launcher.core.LauncherFactory;

final class PlatformTests {
    private PlatformTests() {}

    static boolean run(List<String> selectors, TestRunner.Events events) {
        List<DiscoverySelector> discovery = new ArrayList<>();
        List<String> patterns = new ArrayList<>();
        for (String selector : selectors) {
            if (selector.startsWith("method:")) {
                discovery.add(DiscoverySelectors.selectMethod(
                        TestRunner.className(selector) + "#" + TestRunner.methodName(selector)));
            } else if (selector.startsWith("class:")) {
                discovery.add(DiscoverySelectors.selectClass(TestRunner.className(selector)));
            } else if (selector.startsWith("package:")) {
                discovery.add(DiscoverySelectors.selectPackage(TestRunner.value(selector)));
            } else if (selector.startsWith("pattern:")) {
                patterns.add(".*(" + TestRunner.value(selector) + ").*");
            }
        }
        LauncherDiscoveryRequestBuilder builder = LauncherDiscoveryRequestBuilder.request();
        if (!patterns.isEmpty()) {
            discovery.add(DiscoverySelectors.selectPackage(""));
            builder.filters(ClassNameFilter.includeClassNamePatterns(patterns.toArray(new String[0])));
        }
        LauncherDiscoveryRequest request = builder.selectors(discovery).build();
        Launcher launcher = LauncherFactory.create();
        Listener listener = new Listener(events);
        launcher.execute(request, listener);
        return !listener.failed;
    }

    static final class Listener implements TestExecutionListener {
        private final TestRunner.Events events;
        private final Map<String, String> ids = new ConcurrentHashMap<>();
        private final Map<String, Long> starts = new ConcurrentHashMap<>();
        private TestPlan plan;
        volatile boolean failed;

        Listener(TestRunner.Events events) {
            this.events = events;
        }

        @Override
        public void testPlanExecutionStarted(TestPlan plan) {
            this.plan = plan;
            events.count((int) plan.countTestIdentifiers(TestIdentifier::isTest));
        }

        /** The IDE's id of a node's parent: an engine's node isn't shown, its children are the root's. */
        private String parentOf(TestIdentifier test) {
            String parent = test.getParentId().orElse(null);
            if (parent == null) {
                return "0";
            }
            String id = ids.get(parent);
            return id == null ? "0" : id;
        }

        private String start(TestIdentifier test) {
            String id = events.nextId();
            ids.put(test.getUniqueId(), id);
            starts.put(test.getUniqueId(), System.currentTimeMillis());
            TestSource source = test.getSource().orElse(null);
            if (test.isTest()) {
                String type = "";
                String method = test.getDisplayName();
                if (source instanceof MethodSource) {
                    type = ((MethodSource) source).getClassName();
                    method = ((MethodSource) source).getMethodName();
                }
                events.testStarted(id, parentOf(test), test.getDisplayName(), type, method);
            } else {
                String type = source instanceof ClassSource ? ((ClassSource) source).getClassName() : null;
                events.suiteStarted(id, parentOf(test), test.getDisplayName(), type);
            }
            return id;
        }

        @Override
        public void executionStarted(TestIdentifier test) {
            if (test.getParentId().isPresent()) {
                start(test);
            }
        }

        @Override
        public void executionSkipped(TestIdentifier test, String reason) {
            if (!test.getParentId().isPresent()) {
                return;
            }
            String id = start(test);
            if (test.isTest()) {
                events.testIgnored(id, reason);
                events.testFinished(id, 0);
            } else {
                // a skipped class: each of its tests ignored, as IntelliJ shows it
                for (TestIdentifier child : plan.getChildren(test)) {
                    executionSkipped(child, reason);
                }
                events.suiteFinished(id);
            }
        }

        @Override
        public void executionFinished(TestIdentifier test, TestExecutionResult result) {
            String id = ids.get(test.getUniqueId());
            if (id == null) {
                if (result.getStatus() != TestExecutionResult.Status.SUCCESSFUL) {
                    failed = true;
                }
                return;
            }
            Throwable failure = result.getThrowable().orElse(null);
            switch (result.getStatus()) {
                case SUCCESSFUL:
                    break;
                case ABORTED:
                    if (test.isTest()) {
                        events.testIgnored(id, failure == null ? "" : failure.getMessage());
                    }
                    break;
                default:
                    failed = true;
                    if (test.isTest() && failure != null) {
                        events.testFailed(id, failure);
                    } else if (failure != null) {
                        // a class that failed (its set-up): its failure as a test under it
                        String child = events.nextId();
                        events.testStarted(child, id, "classMethod", "", "classMethod");
                        events.testFailed(child, failure);
                        events.testFinished(child, 0);
                    }
            }
            if (test.isTest()) {
                Long start = starts.get(test.getUniqueId());
                events.testFinished(id, start == null ? 0 : System.currentTimeMillis() - start);
            } else {
                events.suiteFinished(id);
            }
        }
    }
}
