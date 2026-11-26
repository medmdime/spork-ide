package com.example;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

class CalculatorTest {
    @Test
    void adds() {
        assertEquals(5, Calculator.add(2, 3));
    }

    @Test
    void signs() {
        assertEquals("positive", Calculator.sign(4));
        assertEquals("positive", Calculator.sign(2));
        assertEquals("zero", Calculator.sign(0));
    }
}
