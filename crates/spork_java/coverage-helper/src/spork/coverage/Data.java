/*
 * Spork IDE's reader of a coverage run's data, GPL-3.0-or-later.
 *
 * usage: Data <file.ic> [<source map>]
 *
 * Writes the coverage IntelliJ's agent saved (its .ic file, its source map of each class's
 * source file, and the folder of per-test traces beside it when the run tracked them) to stdout
 * as one JSON object, for the IDE to read:
 * {"classes": [{"name", "source", "lines": [{"line", "hits", "status", "method",
 * "jumps": [[true hits, false hits]], "switches": [{"keys", "hits", "default"}]}]}],
 * "tests": {test: {class: [lines]}}}.
 */
package spork.coverage;

import com.intellij.rt.coverage.data.ClassData;
import com.intellij.rt.coverage.data.JumpData;
import com.intellij.rt.coverage.data.LineData;
import com.intellij.rt.coverage.data.ProjectData;
import com.intellij.rt.coverage.data.SwitchData;
import com.intellij.rt.coverage.util.CoverageReport;
import com.intellij.rt.coverage.util.ProjectDataLoader;
import com.intellij.rt.coverage.util.TestTrackingIOUtil;

import java.io.File;
import java.io.IOException;
import java.io.OutputStreamWriter;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;

public final class Data {
    private Data() {
    }

    public static void main(String[] args) throws IOException {
        if (args.length < 1 || args.length > 2) {
            System.err.println("usage: Data <file.ic> [<source map>]");
            System.exit(2);
        }
        File file = new File(args[0]);
        if (!file.isFile()) {
            System.err.println("No coverage data in " + file);
            System.exit(1);
        }
        ProjectData data = ProjectDataLoader.load(file);
        if (args.length == 2 && new File(args[1]).isFile()) {
            CoverageReport.loadAndApplySourceMap(data, new File(args[1]));
        }
        PrintWriter out = new PrintWriter(new OutputStreamWriter(System.out, "UTF-8"));
        out.print("{\"classes\":[");
        List<String> names = new ArrayList<String>(data.getClasses().keySet());
        Collections.sort(names);
        boolean first = true;
        for (String name : names) {
            ClassData classData = data.getClassData(name);
            if (!first) {
                out.print(',');
            }
            first = false;
            writeClass(out, classData);
        }
        out.print("],\"tests\":{");
        writeTests(out, file);
        out.print("}}");
        out.println();
        out.flush();
    }

    private static void writeClass(PrintWriter out, ClassData classData) {
        out.print("{\"name\":");
        out.print(string(classData.getName()));
        out.print(",\"source\":");
        out.print(classData.getSource() == null ? "null" : string(classData.getSource()));
        out.print(",\"lines\":[");
        Object[] lines = classData.getLines();
        boolean first = true;
        if (lines != null) {
            for (Object item : lines) {
                if (!(item instanceof LineData)) {
                    continue;
                }
                if (!first) {
                    out.print(',');
                }
                first = false;
                writeLine(out, (LineData) item);
            }
        }
        out.print("]}");
    }

    private static void writeLine(PrintWriter out, LineData line) {
        out.print("{\"line\":" + line.getLineNumber());
        out.print(",\"hits\":" + line.getHits());
        out.print(",\"status\":" + line.getStatus());
        out.print(",\"method\":");
        out.print(line.getMethodSignature() == null ? "null" : string(line.getMethodSignature()));
        out.print(",\"jumps\":[");
        JumpData[] jumps = line.getJumps();
        if (jumps != null) {
            for (int i = 0; i < jumps.length; i++) {
                if (i > 0) {
                    out.print(',');
                }
                out.print("[" + jumps[i].getTrueHits() + "," + jumps[i].getFalseHits() + "]");
            }
        }
        out.print("],\"switches\":[");
        SwitchData[] switches = line.getSwitches();
        if (switches != null) {
            for (int i = 0; i < switches.length; i++) {
                if (i > 0) {
                    out.print(',');
                }
                out.print("{\"keys\":" + ints(switches[i].getKeys()));
                out.print(",\"hits\":" + ints(switches[i].getHits()));
                out.print(",\"default\":" + switches[i].getDefaultHits() + "}");
            }
        }
        out.print("]}");
    }

    /** The traces of a run that tracked each test, in the folder named after the data file. */
    private static void writeTests(PrintWriter out, File file) throws IOException {
        String fileName = file.getName();
        int dot = fileName.lastIndexOf('.');
        File traces = new File(file.getParentFile(), dot == -1 ? fileName : fileName.substring(0, dot));
        if (!traces.isDirectory()) {
            return;
        }
        Map<String, Map<String, int[]>> tests = TestTrackingIOUtil.loadTestTrackingData(traces);
        boolean first = true;
        for (Map.Entry<String, Map<String, int[]>> test : tests.entrySet()) {
            if (!first) {
                out.print(',');
            }
            first = false;
            out.print(string(test.getKey()));
            out.print(":{");
            boolean firstClass = true;
            for (Map.Entry<String, int[]> classLines : test.getValue().entrySet()) {
                if (!firstClass) {
                    out.print(',');
                }
                firstClass = false;
                out.print(string(classLines.getKey()));
                out.print(':');
                out.print(ints(classLines.getValue()));
            }
            out.print('}');
        }
    }

    private static String ints(int[] values) {
        StringBuilder text = new StringBuilder("[");
        for (int i = 0; i < values.length; i++) {
            if (i > 0) {
                text.append(',');
            }
            text.append(values[i]);
        }
        return text.append(']').toString();
    }

    static String string(String text) {
        StringBuilder quoted = new StringBuilder("\"");
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            switch (c) {
                case '"':
                    quoted.append("\\\"");
                    break;
                case '\\':
                    quoted.append("\\\\");
                    break;
                case '\n':
                    quoted.append("\\n");
                    break;
                case '\r':
                    quoted.append("\\r");
                    break;
                case '\t':
                    quoted.append("\\t");
                    break;
                default:
                    if (c < 0x20) {
                        quoted.append(String.format("\\u%04x", (int) c));
                    } else {
                        quoted.append(c);
                    }
            }
        }
        return quoted.append('"').toString();
    }
}
