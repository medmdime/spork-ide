package com.example;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

class DebuggeeTest {
    @Test
    void measuresANameByItsLength() {
        assertEquals(5, Debuggee.measure("Grace"));
    }
}
