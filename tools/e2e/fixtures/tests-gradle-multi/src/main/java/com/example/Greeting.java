package com.example;

public final class Greeting {
    private Greeting() {
    }

    public static String hello(String name) {
        String greeting = "Hello, " + name + "!";
        return greeting;
    }
}
