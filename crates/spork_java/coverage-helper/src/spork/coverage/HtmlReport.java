/*
 * Spork IDE's HTML coverage report, GPL-3.0-or-later.
 *
 * usage: HtmlReport <folder> <title> --reports <file.ic>... --classes <folder>... --sources <folder>...
 *
 * Writes the report IntelliJ's Generate Coverage Report… writes for its own engine, with its
 * reporter (intellij-coverage-reporter), into the folder.
 */
package spork.coverage;

import com.intellij.rt.coverage.report.api.Filters;
import com.intellij.rt.coverage.report.api.ReportApi;

import java.io.File;
import java.io.IOException;
import java.util.ArrayList;
import java.util.List;

public final class HtmlReport {
    private HtmlReport() {
    }

    public static void main(String[] args) throws IOException {
        if (args.length < 2) {
            System.err.println("usage: HtmlReport <folder> <title> --reports <file.ic>... --classes <folder>... --sources <folder>...");
            System.exit(2);
        }
        List<File> reports = new ArrayList<File>();
        List<File> classes = new ArrayList<File>();
        List<File> sources = new ArrayList<File>();
        List<File> current = reports;
        for (int i = 2; i < args.length; i++) {
            if ("--reports".equals(args[i])) {
                current = reports;
            } else if ("--classes".equals(args[i])) {
                current = classes;
            } else if ("--sources".equals(args[i])) {
                current = sources;
            } else {
                current.add(new File(args[i]));
            }
        }
        ReportApi.htmlReport(new File(args[0]), args[1], "UTF-8", reports, classes, sources, Filters.EMPTY);
    }
}
