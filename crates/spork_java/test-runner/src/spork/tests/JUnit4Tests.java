/* Spork IDE's test runner, JUnit 4's part: JUnitCore with a listener. GPL-3.0-or-later. */
package spork.tests;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

import org.junit.runner.Description;
import org.junit.runner.JUnitCore;
import org.junit.runner.Request;
import org.junit.runner.Result;
import org.junit.runner.manipulation.Filter;
import org.junit.runner.notification.Failure;
import org.junit.runner.notification.RunListener;

final class JUnit4Tests {
    private JUnit4Tests() {}

    static boolean run(List<String> selectors, TestRunner.Events events) {
        List<Class<?>> classes = new ArrayList<>();
        final List<String> methods = new ArrayList<>();
        for (String selector : selectors) {
            if (selector.startsWith("class:") || selector.startsWith("method:")) {
                Class<?> type = TestRunner.load(TestRunner.className(selector));
                if (type == null) {
                    System.err.println("Class not found: \"" + TestRunner.className(selector) + "\"");
                    return false;
                }
                if (!classes.contains(type)) {
                    classes.add(type);
                }
                String method = TestRunner.methodName(selector);
                methods.add(method == null ? type.getName() : type.getName() + "#" + method);
            }
        }
        for (Class<?> type : TestRunner.classesIn(selectors)) {
            if ("junit4".equals(TestRunner.frameworkOf(type)) && !classes.contains(type)) {
                classes.add(type);
                methods.add(type.getName());
            }
        }
        Request request = Request.classes(classes.toArray(new Class<?>[0])).filterWith(new Filter() {
            @Override
            public boolean shouldRun(Description description) {
                if (description.getMethodName() == null) {
                    return true;
                }
                String type = description.getClassName();
                return methods.contains(type) || methods.contains(type + "#" + description.getMethodName());
            }

            @Override
            public String describe() {
                return "the selected tests";
            }
        });
        JUnitCore core = new JUnitCore();
        Listener listener = new Listener(events);
        core.addListener(listener);
        events.count(request.getRunner().getDescription().testCount());
        Result result = core.run(request);
        listener.closeSuite();
        return result.wasSuccessful();
    }

    /** Each test under its class's suite, the suites opened as their tests start. */
    static final class Listener extends RunListener {
        private final TestRunner.Events events;
        private final Map<Description, String> ids = new HashMap<>();
        private final Map<Description, Long> starts = new HashMap<>();
        private String suiteClass;
        private String suiteId;

        Listener(TestRunner.Events events) {
            this.events = events;
        }

        private String suite(Description test) {
            String type = test.getClassName();
            if (!type.equals(suiteClass)) {
                closeSuite();
                suiteClass = type;
                suiteId = events.nextId();
                int dot = type.lastIndexOf('.');
                events.suiteStarted(suiteId, "0", type.substring(dot + 1), type);
            }
            return suiteId;
        }

        void closeSuite() {
            if (suiteId != null) {
                events.suiteFinished(suiteId);
                suiteId = null;
                suiteClass = null;
            }
        }

        private String start(Description test) {
            String parent = suite(test);
            String id = events.nextId();
            ids.put(test, id);
            starts.put(test, System.currentTimeMillis());
            String method = test.getMethodName() == null ? test.getDisplayName() : test.getMethodName();
            events.testStarted(id, parent, method, test.getClassName(), method);
            return id;
        }

        @Override
        public void testStarted(Description test) {
            start(test);
        }

        @Override
        public void testFinished(Description test) {
            String id = ids.get(test);
            Long start = starts.get(test);
            if (id != null) {
                events.testFinished(id, start == null ? 0 : System.currentTimeMillis() - start);
            }
        }

        @Override
        public void testFailure(Failure failure) {
            String id = ids.get(failure.getDescription());
            if (id == null) {
                // a class that couldn't start: its failure as a test of its own
                id = start(failure.getDescription());
                events.testFailed(id, failure.getException());
                events.testFinished(id, 0);
                return;
            }
            events.testFailed(id, failure.getException());
        }

        @Override
        public void testAssumptionFailure(Failure failure) {
            String id = ids.get(failure.getDescription());
            if (id != null) {
                events.testIgnored(id, failure.getMessage());
            }
        }

        @Override
        public void testIgnored(Description test) {
            String id = start(test);
            org.junit.Ignore ignore = test.getAnnotation(org.junit.Ignore.class);
            events.testIgnored(id, ignore == null ? "" : ignore.value());
            events.testFinished(id, 0);
        }
    }
}
