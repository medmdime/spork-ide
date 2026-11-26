package com.example;

/** The greeting {@link Main} prints. */
public final class Greeting {
    private Greeting() {
    }

    public static String hello(String name) {
        return "Hello, " + name + "!";
    }
}
