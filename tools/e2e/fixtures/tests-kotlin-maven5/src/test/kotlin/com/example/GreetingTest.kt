package com.example

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test

class GreetingTest {
    @Test
    fun greets() {
        assertEquals("Hello, world!", greet("world"))
    }

    @Test
    fun greetsAda() {
        assertEquals("Hello, Ada!", greet("world"))
    }
}
