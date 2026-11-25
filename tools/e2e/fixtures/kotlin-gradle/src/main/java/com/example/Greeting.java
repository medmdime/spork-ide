package com.example;

/** A greeting for someone. */
public class Greeting {
    private final String name;

    public Greeting(String name) {
        this.name = name;
    }

    public String text() {
        return "Hello, " + name + "!";
    }
}
