package com.example.app;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

class AppTest {
    @Test
    void countsLetters() {
        assertEquals(3, Counter.count("tea"));
    }
}
