package com.example;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

class GreetingTest {
    @Test
    void greetsAda() {
        assertEquals("Hello, Ada!", Greeting.hello("Ada"));
    }
}
