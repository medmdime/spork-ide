package com.example;

import static org.testng.Assert.assertEquals;

import org.testng.annotations.Test;

public class OrderTest {
    @Test
    public void countsItems() {
        assertEquals(java.util.List.of("tea", "milk").size(), 2);
    }
}
