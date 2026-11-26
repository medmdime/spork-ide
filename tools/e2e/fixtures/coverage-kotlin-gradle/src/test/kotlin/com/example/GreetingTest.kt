package com.example

import kotlin.test.Test
import kotlin.test.assertEquals

class GreetingTest {
    @Test
    fun greets() {
        val greeting = greet("world")
        assertEquals("Hello, world!", greeting)
    }

    @Test
    fun greetsAda() {
        assertEquals("Hello, Ada!", greet("Ada"))
    }
}
