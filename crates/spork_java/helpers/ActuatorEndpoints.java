// Spork IDE: reads a running Spring Boot application's Actuator endpoints over JMX, as IntelliJ's
// Actuator tab does: attaches to the JVM whose `spork.boot.process` property is the run's marker
// (the Attach API), starts its local management agent and calls the endpoint MBeans. Run with the
// application's JDK as a single-file program: `java ActuatorEndpoints.java <main class> <marker>`.
// It prints one JSON object: each endpoint's data by its id, `missing` the endpoints not there.

import com.sun.tools.attach.VirtualMachine;
import com.sun.tools.attach.VirtualMachineDescriptor;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Set;
import javax.management.MBeanServerConnection;
import javax.management.ObjectName;
import javax.management.remote.JMXConnector;
import javax.management.remote.JMXConnectorFactory;
import javax.management.remote.JMXServiceURL;

public class ActuatorEndpoints {

    public static void main(String[] args) throws Exception {
        VirtualMachine vm = find(args[0], args[1]);
        if (vm == null) {
            System.out.println("{\"error\": \"Could not find application process to attach\"}");
            return;
        }
        String address = vm.startLocalManagementAgent();
        vm.detach();
        try (JMXConnector connector = JMXConnectorFactory.connect(new JMXServiceURL(address))) {
            MBeanServerConnection server = connector.getMBeanServerConnection();
            // IntelliJ's readiness check: Spring Boot's admin MBean says the application is ready
            ObjectName admin = new ObjectName("org.springframework.boot:type=Admin,name=SpringApplication");
            if (!server.isRegistered(admin) || !Boolean.TRUE.equals(server.getAttribute(admin, "Ready"))) {
                System.out.println("{\"error\": \"Application readiness check failed: Check that the application started successfully.\"}");
                return;
            }
            Set<ObjectName> endpoints = server.queryNames(new ObjectName("org.springframework.boot:type=Endpoint,*"), null);
            StringBuilder out = new StringBuilder("{");
            List<String> missing = new ArrayList<>();
            String[][] reads = {
                {"beans", "Beans", "beans"},
                {"health", "Health", "health"},
                {"mappings", "Mappings", "mappings"},
                {"env", "Env", "environment"},
            };
            for (String[] read : reads) {
                ObjectName name = null;
                for (ObjectName endpoint : endpoints) {
                    if (read[1].equals(endpoint.getKeyProperty("name"))) {
                        name = endpoint;
                    }
                }
                if (name == null) {
                    missing.add(read[0]);
                    continue;
                }
                Object data = read[0].equals("env")
                    ? server.invoke(name, read[2], new Object[] {null}, new String[] {"java.lang.String"})
                    : server.invoke(name, read[2], new Object[0], new String[0]);
                out.append(json(read[0])).append(": ");
                write(out, data);
                out.append(", ");
            }
            out.append("\"missing\": ");
            write(out, missing);
            System.out.println(out.append("}"));
        }
    }

    /** The JVM running `main` whose marker is `marker`; the only JVM running it without one. */
    static VirtualMachine find(String main, String marker) {
        for (VirtualMachineDescriptor descriptor : VirtualMachine.list()) {
            if (!descriptor.displayName().startsWith(main)) {
                continue;
            }
            try {
                VirtualMachine vm = VirtualMachine.attach(descriptor);
                if (marker.equals(vm.getSystemProperties().getProperty("spork.boot.process"))) {
                    return vm;
                }
                vm.detach();
            } catch (Exception ignored) {
                // a JVM that ended or doesn't allow attaching isn't the application's
            }
        }
        return null;
    }

    static void write(StringBuilder out, Object value) {
        if (value == null) {
            out.append("null");
        } else if (value instanceof Map<?, ?> map) {
            out.append("{");
            boolean first = true;
            for (Map.Entry<?, ?> entry : map.entrySet()) {
                out.append(first ? "" : ", ").append(json(String.valueOf(entry.getKey()))).append(": ");
                write(out, entry.getValue());
                first = false;
            }
            out.append("}");
        } else if (value instanceof Iterable<?> list) {
            out.append("[");
            boolean first = true;
            for (Object item : list) {
                out.append(first ? "" : ", ");
                write(out, item);
                first = false;
            }
            out.append("]");
        } else if (value instanceof Object[] array) {
            write(out, List.of(array));
        } else if (value instanceof Number || value instanceof Boolean) {
            out.append(value);
        } else {
            out.append(json(String.valueOf(value)));
        }
    }

    static String json(String text) {
        StringBuilder out = new StringBuilder("\"");
        for (char c : text.toCharArray()) {
            switch (c) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (c < 0x20) {
                        out.append(String.format("\\u%04x", (int) c));
                    } else {
                        out.append(c);
                    }
                }
            }
        }
        return out.append('"').toString();
    }
}
