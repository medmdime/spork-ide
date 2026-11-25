package com.example;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Disabled;
import org.junit.jupiter.api.Test;

class CalculatorTest {
    @Test
    void adds() {
        System.out.println("adding 2 and 3");
        assertEquals(5, 2 + 3);
    }

    @Test
    void greetsAda() {
        assertEquals("Hello, Ada!", Greeting.hello("world"));
    }

    @Disabled("not yet")
    @Test
    void divides() {
        assertEquals(2, 4 / 2);
    }
}
