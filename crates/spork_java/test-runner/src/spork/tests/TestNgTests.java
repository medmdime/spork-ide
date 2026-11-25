/* Spork IDE's test runner, TestNG's part. GPL-3.0-or-later. */
package spork.tests;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import org.testng.ITestContext;
import org.testng.ITestListener;
import org.testng.ITestResult;
import org.testng.TestNG;
import org.testng.xml.XmlClass;
import org.testng.xml.XmlInclude;
import org.testng.xml.XmlSuite;
import org.testng.xml.XmlTest;

final class TestNgTests {
    private TestNgTests() {}

    static boolean run(List<String> selectors, TestRunner.Events events) {
        // the selected classes, each with its selected methods (none: all of them)
        Map<String, List<String>> classes = new LinkedHashMap<>();
        for (String selector : selectors) {
            if (selector.startsWith("class:") || selector.startsWith("method:")) {
                String method = TestRunner.methodName(selector);
                List<String> methods = classes.computeIfAbsent(TestRunner.className(selector), k -> new ArrayList<>());
                if (method != null) {
                    methods.add(method);
                }
            }
        }
        for (Class<?> type : TestRunner.classesIn(selectors)) {
            if ("testng".equals(TestRunner.frameworkOf(type))) {
                classes.putIfAbsent(type.getName(), new ArrayList<String>());
            }
        }
        XmlSuite suite = new XmlSuite();
        suite.setName("Default Suite");
        XmlTest test = new XmlTest(suite);
        test.setName("Default test");
        List<XmlClass> xmlClasses = new ArrayList<>();
        for (Map.Entry<String, List<String>> entry : classes.entrySet()) {
            XmlClass xmlClass = new XmlClass(entry.getKey());
            List<XmlInclude> includes = new ArrayList<>();
            for (String method : entry.getValue()) {
                includes.add(new XmlInclude(method));
            }
            xmlClass.setIncludedMethods(includes);
            xmlClasses.add(xmlClass);
        }
        test.setXmlClasses(xmlClasses);
        TestNG testng = new TestNG();
        testng.setUseDefaultListeners(false);
        testng.setXmlSuites(Collections.singletonList(suite));
        Listener listener = new Listener(events);
        testng.addListener(listener);
        testng.run();
        listener.closeSuite();
        return !testng.hasFailure();
    }

    static final class Listener implements ITestListener {
        private final TestRunner.Events events;
        private final Map<ITestResult, String> ids = new LinkedHashMap<>();
        private String suiteClass;
        private String suiteId;

        Listener(TestRunner.Events events) {
            this.events = events;
        }

        @Override
        public void onStart(ITestContext context) {
            events.count(context.getAllTestMethods().length);
        }

        private String suite(String type) {
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

        private String id(ITestResult result) {
            String id = ids.get(result);
            if (id == null) {
                String type = result.getTestClass().getName();
                String parent = suite(type);
                id = events.nextId();
                ids.put(result, id);
                String method = result.getMethod().getMethodName();
                events.testStarted(id, parent, method, type, method);
            }
            return id;
        }

        private void finish(ITestResult result) {
            events.testFinished(id(result), Math.max(result.getEndMillis() - result.getStartMillis(), 0));
        }

        @Override
        public void onTestStart(ITestResult result) {
            id(result);
        }

        @Override
        public void onTestSuccess(ITestResult result) {
            finish(result);
        }

        @Override
        public void onTestFailure(ITestResult result) {
            if (result.getThrowable() != null) {
                events.testFailed(id(result), result.getThrowable());
            }
            finish(result);
        }

        @Override
        public void onTestSkipped(ITestResult result) {
            Throwable reason = result.getThrowable();
            events.testIgnored(id(result), reason == null ? "" : reason.getMessage());
            finish(result);
        }

        @Override
        public void onTestFailedButWithinSuccessPercentage(ITestResult result) {
            finish(result);
        }

        @Override
        public void onFinish(ITestContext context) {}
    }
}
