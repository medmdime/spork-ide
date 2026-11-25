package com.example;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/** The debugger's program: a loop, a field, a record, a thread, a lambda and a caught exception. */
public class Debuggee {
    static int counter;

    record Point(int x, int y) {
    }

    public static void main(String[] args) throws IOException, InterruptedException {
        List<String> names = List.of("Ada", "Grace", "Linus");
        int total = 0;
        for (int i = 0; i < names.size(); i++) {
            total += measure(names.get(i));
        }
        Map<String, Integer> lengths = new HashMap<>();
        names.forEach(name -> lengths.put(name, name.length()));
        Point point = new Point(1, 2);
        int sum = total;
        Thread worker = new Thread(() -> System.out.println("worker " + twice(sum)), "worker");
        worker.start();
        worker.join();
        try {
            Integer.parseInt("x");
        } catch (NumberFormatException e) {
            System.out.println("not a number");
        }
        if (args.length > 0 && args[0].equals("--ask")) {
            System.out.print("Name? ");
            String name = new BufferedReader(new InputStreamReader(System.in)).readLine();
            System.out.println("Hello, " + name + "!");
        }
        System.out.println(total + " " + lengths.size() + " " + point);
    }

    static int measure(String name) {
        counter += name.length();
        return name.length();
    }

    static int twice(int value) {
        return value * 2;
    }
}
