package com.example;

/** What the tests cover: a branch taken both ways, a switch, a line no test reaches. */
public final class Calculator {
    private Calculator() {
    }

    public static int add(int a, int b) {
        return a + b;
    }

    public static String sign(int number) {
        if (number > 0) {
            return "positive";
        }
        switch (number) {
            case 0:
                return "zero";
            case -1:
                return "minus one";
            default:
                return "negative";
        }
    }

    public static int divide(int a, int b) {
        return a / b;
    }
}
