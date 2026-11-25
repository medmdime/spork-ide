package com.example;

import static org.junit.Assert.assertEquals;

import org.junit.Test;

public class DebuggeeTest {
    @Test
    public void measuresANameByItsLength() {
        assertEquals(5, Debuggee.measure("Grace"));
    }
}
