/* Spork IDE's test runner, IntelliJ's fork modes: each test class or method in a JVM of its own,
 * one after the other, each reporting its events to the IDE. GPL-3.0-or-later. */
package spork.tests;

import java.io.File;
import java.lang.annotation.Annotation;
import java.lang.management.ManagementFactory;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

final class Forks {
    private Forks() {}

    /** Runs the selected tests a class (`class`) or a method (`method`) per JVM; whether all passed. */
    static boolean run(String mode, List<String> selectors) throws Exception {
        // the selected classes, each with its selected methods (none: all of them)
        Map<String, List<String>> classes = new LinkedHashMap<>();
        List<String> others = new ArrayList<>();
        for (String selector : selectors) {
            if (selector.startsWith("class:") || selector.startsWith("method:")) {
                List<String> methods = classes.computeIfAbsent(TestRunner.className(selector), k -> new ArrayList<>());
                String method = TestRunner.methodName(selector);
                if (method != null) {
                    methods.add(method);
                }
            } else {
                others.add(selector);
            }
        }
        for (Class<?> type : TestRunner.classesIn(selectors)) {
            if (TestRunner.frameworkOf(type) != null) {
                classes.putIfAbsent(type.getName(), new ArrayList<String>());
            }
        }
        List<List<String>> units = new ArrayList<>();
        for (Map.Entry<String, List<String>> entry : classes.entrySet()) {
            List<String> methods = entry.getValue();
            if (mode.equals("method")) {
                if (methods.isEmpty()) {
                    methods = testMethods(TestRunner.load(entry.getKey()));
                }
                for (String method : methods) {
                    units.add(List.of("method:" + entry.getKey() + "#" + method));
                }
            } else {
                List<String> unit = new ArrayList<>();
                if (methods.isEmpty()) {
                    unit.add("class:" + entry.getKey());
                }
                for (String method : methods) {
                    unit.add("method:" + entry.getKey() + "#" + method);
                }
                units.add(unit);
            }
        }
        // the kinds that select no classes (tags, a unique id, a suite) run in one JVM
        if (units.isEmpty()) {
            units.add(others);
        }
        String runner = new File(TestRunner.class.getProtectionDomain().getCodeSource().getLocation().toURI()).getPath();
        String classpath = runner + File.pathSeparator + System.getProperty("java.class.path");
        String java = System.getProperty("java.home") + File.separator + "bin" + File.separator + "java";
        // the JVM's options, without its debugger's agent: the forks would wait for one
        List<String> options = new ArrayList<>();
        for (String option : ManagementFactory.getRuntimeMXBean().getInputArguments()) {
            if (!option.startsWith("-agentlib:jdwp") && !option.startsWith("-Xrunjdwp")) {
                options.add(option);
            }
        }
        boolean passed = true;
        int fork = 0;
        for (List<String> unit : units) {
            List<String> command = new ArrayList<>();
            command.add(java);
            command.addAll(options);
            command.add("-classpath");
            command.add(classpath);
            command.add(TestRunner.class.getName());
            command.addAll(unit);
            ProcessBuilder builder = new ProcessBuilder(command).inheritIO();
            builder.environment().put(TestRunner.FORKED, "f" + (++fork) + "-");
            passed &= builder.start().waitFor() == 0;
        }
        return passed;
    }

    /** A class's test methods, by their frameworks' annotations. */
    static List<String> testMethods(Class<?> type) {
        List<String> methods = new ArrayList<>();
        if (type == null) {
            return methods;
        }
        for (Method method : type.getDeclaredMethods()) {
            for (Annotation annotation : method.getAnnotations()) {
                String name = annotation.annotationType().getName();
                if (name.endsWith(".Test") || name.endsWith(".ParameterizedTest") || name.endsWith(".RepeatedTest")
                        || name.endsWith(".TestFactory") || name.endsWith(".TestTemplate")) {
                    if (!methods.contains(method.getName())) {
                        methods.add(method.getName());
                    }
                    break;
                }
            }
        }
        return methods;
    }
}
