package com.example;

import static org.junit.Assert.assertEquals;

import org.junit.Test;

public class GreetingTest {
    @Test
    public void greetsByName() {
        assertEquals("Hello, world!", Greeting.hello("world"));
    }
}
