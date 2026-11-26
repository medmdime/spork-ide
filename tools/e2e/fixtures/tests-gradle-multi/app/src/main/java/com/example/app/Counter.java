package com.example.app;

public final class Counter {
    private Counter() {
    }

    public static int count(String text) {
        return text.length();
    }
}
